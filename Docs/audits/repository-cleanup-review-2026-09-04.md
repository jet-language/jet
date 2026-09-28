# Repository cleanup review

**Status: the approved preservation-first cleanup is complete on card #2932.** Cards #2928–#2931 remain the completed preparation, AGENTS editor, and B7 skill work. The execution record below supersedes the proposed actions and approval holds retained later in this report.

## Completed execution

| Batch | Delivered result |
|---|---|
| B1 | Removed the exact verified root detritus and obsolete wrapper. Retained active caches and working source. Moved failed parity evidence intact to `docs/audits/jit-aot-parity-2026-08-31/` and recorded its unresolved failures on #2919. Preserved the four ignored literal-tilde evidence files under `docs/audits/domain-dx2/`. |
| B2 | Only `spec/`, `audits/`, `research/`, and `proposals/` remain under docs, plus the navigation README. References, guides, packaging contracts, and contributor instructions have current destinations and migrated consumers. |
| B3 | Preserved decisions, reasons, alternatives, constraints, evidence, and live-state boundaries in proposal decisions, domain-foundations history/method, and compiled-workload source-boundary notes. Kept unresolved designs and investigations. |
| B4 | Root AGENTS.md is the sole shared contract. Orchestration is `.agents/skills/orchestration/SKILL.md`. Nine old policy/procedure sources passed exact identity checks before retirement. Technical traps and preservation mappings remain in contributor engineering notes. |
| B5 | Retained protected reports, raw payloads, HTML companions, and prompt inputs under Audits. Required path-only link repairs are separate from historical findings; no report was deleted or summarized away. |
| B6 | Moved the DX manifest and research executables to tools/agent-eval. All 611 domain-foundations fixture files remain byte-identical. Updated active consumers and retired the old locations. |
| B7 | Retained both ratified skill outcomes. Added the canonical orchestration entry to the maintained map and migrated its checker to root policy and the skill path. |

### Exact recovery

- `cleanup-source-2026-09-05-114107`: all **1,286 captured source identities** match the preserved Git tree `6875b5e07a55900a630f280c9c88edbfca2094b1`.
- `cleanup-root-source-2026-09-05-120351-complete`: exact root/evidence preservation tree `98f47304eb84b22066bb75a7ece93d940f619304`.
- Recover an original with `git show cleanup-source-2026-09-05-114107:<original-path>`. The original path inventories below and the topic preservation maps identify the source.
- Session receipts: `local://cleanup-verification.json`, `local://cleanup-root-receipt.json`, `local://cleanup-policy-retirement.json`, `local://cleanup-document-links.json`, and the slice receipts named in the handoff. The Git references, not ephemeral session paths, are the durable recovery mechanism.

### Exercised proof and limits

- The three focused Tower Docs, server, and security modules passed **36 tests, zero failures**. This covers document operations, four-section listing, AGENTS revisions, hostile paths, and unchanged scratch behavior.
- The real Docs CLI lists only Spec, Audits, Research, and Proposals. The current owner server rendered the four categories, opened the retained domain-history note, and loaded root AGENTS. Desktop and 390-pixel navigation were exercised. The mobile long-filename collapse found during that check was corrected.
- **226 active Markdown link destinations across 90 files resolve.** The one absent historical type-unification report is now identified as an unavailable historical citation, not a broken link or invented report.
- Fresh-agent preflight returned `CHECK OK`: 12 domains, both configured adapter commands available, no existing output. Its missing Node assert import was fixed; papercut `pc0njn9m4` records the defect.
- All four relocated bounded JavaScript demonstrator models executed. Relocated executables and the changed preflight/checker passed Node syntax checks. This is not Jet compiler or runtime qualification.
- The DX matrix still rejects an existing missing `unreal` profile; papercut `pc024ddgj` records it. No benchmark win is claimed.
- Whole-host skill inventory stops at the external managed skill `implementation-before-validation`, which is absent from its maintained map. No global or managed skill source was changed to hide that failure.
- The historical audit census reports 65 ledger errors, including missing disposition tables and old Tower references. Protected historical reports were retained rather than rewritten to manufacture a green census.
- No compiler suite, benchmark qualification, milestone/release gate, commit, or owner visual acceptance is claimed. Unrelated shared-tree work was not staged or reverted.

## Reviewed proposal retained as history

The following plan and inventory record the original review, including holds that were resolved by the approved execution above. They are not a second current work ledger. Runtime failures and unresolved product work remain open on their owning Tower cards.


## What this review recommends

Keep useful project inputs. Remove accidental output only after the approved preservation check. Collapse repeated policy, not the safeguards it contains. Keep distinct audit methods unless a substantive ballot establishes a better replacement.

The owner approved exactly four documentation categories: `docs/spec/`, `docs/audits/`, `docs/research/`, and `docs/proposals/`. This is a one-time cleanup. It does not add a root allowlist, recurring janitor, retention daemon, new archive, or new administrative ledger. Working compiler and standard-library source layouts are outside scope.

**At the review stage, nothing in an unapproved batch had been moved, distilled, or deleted.** Audits and final reports remain owner-removable only. Agents may organize them only in an approved move batch. Git recovery is not a substitute for preserving the information readers still need.

### Review order

| Batch | Proposed result | Approval boundary |
|---|---|---|
| B1 | Remove identified root detritus; retain real inputs and failed evidence | Exact paths only; no recursive cleanup |
| B2 | Put durable documentation under spec subfolders; leave its meaning intact | Approve destination and consumer migration together |
| B3 | Distill completed working material into topic notes or decisions | Resolve every knowledge and status gate before removal |
| B4 | Replace overlapping agent-policy documents with one short contract and scoped skills | Review the rule-to-destination map; approve behavioral changes explicitly |
| B5 | Organize protected reports without removing them | Owner decides when to remove the originals |
| B6 | Move executable benchmark/research inputs out of docs where warranted | Preserve fixtures, manifests, evidence identities, and every caller |
| B7 | Improve skill contracts and composition | Substantive Tower ballots; no merge based on similar names alone |

## Agreed final organization

| Home | Meaning |
|---|---|
| `docs/spec/` | Current durable Jet knowledge. Named subfolders distinguish language rules, reference, user guides, contributor instructions, and the concise decision index. |
| `docs/audits/` | Audits and final reports. A finished report remains here until the owner removes it. This temporary review belongs here too. |
| `docs/research/` | Active investigations and reusable topic-level distillations. Keep findings and evidence; do not retain one summary per old working file by default. |
| `docs/proposals/` | Unresolved designs. Ratified meaning moves to the relevant spec; implementation work lives in Tower. Rejected and superseded reasoning remains discoverable through the decision index. |

A concise `docs/README.md` may navigate these four categories. It is an index, not a fifth category. Product examples, test fixtures, executable tools, and tool-required README files keep their functional homes outside docs. Existing working roots such as `Source/`, `crates/`, `tests/`, and `corelib/` are not being renamed for cosmetic consistency.

## Preservation rule for every substantive removal

1. Identify the document's distinct conclusions, reasons, rejected alternatives, constraints, caveats, evidence, and reusable lessons.
2. Resolve or transfer open questions. A date, filename, done heading, or agent completion claim does not establish completion.
3. Give each retained fact one maintained destination. Accepted behavior belongs in its topical spec; work status belongs in Tower; reusable research belongs in a topic note.
4. Preserve the full original with a precise Git reference. A dirty or untracked source is not recoverable from the recorded HEAD merely because its path appears in the report.
5. Check the proposed distillation against the source, preserving contradictions explicitly. Update links, generators, manifests, and other consumers before removing the replaced file.
6. Obtain approval for the exact batch. Recheck source identity and active ownership at execution. A changed or newly active source returns to HOLD.

No source is proposed for deletion merely to make a count fall. A numerical receipt, benchmark corpus, raw evidence table, or executable reproducer is not interchangeable with an LLM summary.

## Evidence and coverage

Inventory captured at 2026-09-05T00:01:57.258Z: **344 distinct Markdown/HTML paths**. This filesystem census includes historical and research material outside the earlier Tower view. It does not claim that Tower displays this count.

Recovery baseline: `4df728d30bdc4c6317a04898967a39132e4f7538`. At capture, 327 paths were tracked and 30 were changed or untracked. All 15 bounded inventory queries returned without truncation.

This is complete path coverage for the listed extensions, not a claim that every paragraph has been read. Substantive findings name their evidence below. Family dispositions are provisional. Active, unread, contradictory, and unpreserved material stays on hold.

| Family | Markdown/HTML paths |
|---|---:|
| `docs/ (direct files)` | 3 |
| `docs/spec` | 19 |
| `docs/reference` | 46 |
| `docs/research` | 137 |
| `docs/proposals` | 40 |
| `docs/plans` | 18 |
| `docs/agents` | 12 |
| `docs/audits` | 43 |
| `docs/infra` | 3 |
| `docs/design` | 1 |
| `docs/ballots` | 0 |
| `docs/continuity` | 1 |
| `docs/archive` | 17 |
| `docs/sidequests` | 4 |
| `docs/benchmarks` | 0 |

Non-prose coverage: `docs/spec/feature-claims.json`; numerical JSON/TSV audit receipts; `docs/benchmarks/dx/{manifest.json,run.mjs}`; executable research fixtures and scripts; proposal screenshots and companion media. These remain functional inputs or evidence. No removal is proposed without a producer/consumer and preservation check.

The appendix records the precise path set. It excludes this newly authored review itself.

## B1: repository-root dispositions

| Source | Proposed action | Information or function preserved | Gate |
|---|---|---|---|
| `.agent-worktrees/` | Remove only if still empty | Existing in-repo worktree support and actual worktrees remain untouched | Approve B1; recheck emptiness and ownership |
| `~/` | Remove the empty literal directory | No entries were present; this is not the user's home directory | Approve B1; recheck emptiness |
| `result` | Remove this symlink only | Its target is `/nix/store/9kmg76nzfkr6zkbxs6kc2pfydp4jzlhq-codex-0.153.0`; installed tools and the store are not removed | Approve B1 |
| `rustc-args` | Remove the stray four-byte dump | Its complete content is `-Vv` followed by a newline | Approve B1 |
| `rustc-stderr` | Remove the empty dump | Zero-byte file; no diagnostic text to preserve | Approve B1 |
| `local:/rights-card.json` | Remove this exact duplicate after rechecking; then remove the parent only if empty | Snapshot #2501, updated 2026-09-04T00:19:58.832Z. Every stored key matched the live Tower card during inspection, including plan, log, criteria, and claim | Approve B1; compare again if either side changes |
| `jit-aot-parity-report/` | HOLD, not a bulk-delete candidate | `result.txt` says `failed`. Receipt identifies commit `e3b5362fcb8c3df6e8f668bf42a12101960649fd`, master, Linux x86-64, 2026-08-31T23:22:50Z | Identify and preserve the failed proof's owning card and required raw evidence before proposing removal |
| `__snapshots__/` | HOLD pending exact fixture attribution | Ignored root snapshot output is not automatically an authoritative fixture, but its producer and relevance must be established | A later exact-path batch must distinguish stray output from useful regression evidence |
| `llms.text` | KEEP | Checked-in generated `jet inspect digest` output, used by the cold-context benchmark control and consumer tests. It has current uncommitted changes | Not random prose; moving or deleting it would change documented consumers |
| `.jet/`, `target/`, `.claude/worktrees/`, `.claude` recovery/output | HOLD outside B1 | Active compiler caches, receipts, worktrees, and recovery evidence may be in use | No blanket cache purge or destructive cleanup is authorized |
| `AGENTS.md`, `CLAUDE.md`, `.agents/`, `.claude/`, plugin manifests, `skills-lock.json` | KEEP pending B4/B7 | Canonical policy, its cross-tool link, hooks, skill discovery, plugin inputs, and imported-skill metadata | Consolidate only after the consumer/authority map and ballots are approved |
| `Cargo.toml`, `Cargo.lock`, `build.rs`, `flake.nix`, `flake.lock`, `.envrc`, `.gitignore`, `clippy.toml`, `env.jet` | KEEP | Build, dependency, environment, and tool configuration | No cosmetic relocation of functional inputs |
| `Source/`, `crates/`, `corelib/`, `tests/`, `examples/`, `templates/`, `editors/`, `plugins/`, `scripts/`, `tools/`, `gauntlet/`, `site/`, `assets/`, `adoption/`, `dogfood/`, `.github/`, `.zed/` | KEEP | Working project components and their local tooling | The owner selected content cleanup, not a source-layout refactor |
| `README.md`, `LICENSE` | KEEP | Project entry point and license | Update navigation only when approved moves require it |

The `.gitignore` already names the root snapshot output, stray rustc dumps, and literal `~/`. Ignoring a path keeps it out of Git; it does not make its contents safe to erase. B1 adds no ongoing prevention policy.

## B2: move durable knowledge without changing its authority

This batch changes filing, not Jet semantics. Each source moves only with its links and automated consumers. Consolidating sections within these files requires a separate approved content map.

| Source | Proposed destination/action | Knowledge that must survive | Evidence and gate |
|---|---|---|---|
| `docs/spec/architecture.md`, `tir.md` | Keep as canonical specifications | Front end owns meaning; one BuildPlan/store; one semantic core across AOT, Cranelift, interpreter, and web | Architecture decisions D-STRUCT-PLANE1, D-COMPILERSEAMS1/2, D-BUILD-STORE1, D-FRONTENDAPI1; TIR #779/#2301. Do not turn implementation gaps into weaker law |
| `docs/spec/syntax-decisions.md` | Keep current law; propose a small navigational decision index, not another authority | Ratified wording, acceptance terms, rationale, supersession links, and decision IDs | The source is 9,315 lines. Length alone does not justify removing a ruling |
| `docs/spec/encoding-decisions.md`, `proof-replay-decisions.md`, `performance-budget-decisions.md` | Keep; consider topical consolidation only after a ruling-to-section map | Encoding law; ProofReport/artifact/replay states; budget/report/content-addressed-store rules | Encoding #296 explicitly distinguishes ratification from implementation; proof/replay #240. Preserve that distinction |
| `docs/spec/release-policy.md`, `versioning.md` | Keep policy; separate it from claims about current qualification | Release and versioning obligations | #2927 and current trust research expose readiness questions. A prerelease implementation does not itself repeal ratified policy |
| `docs/spec/diagnostic-rows.md` | Keep as a generated projection | Diagnostic index and its source relationship | `Diagnostics.jet` is authoritative. Move or regenerate only through the existing producer |
| `docs/first-hour.md`, `diagnostic-recovery.md` | `docs/spec/guides/` | New → run → check → test → fix → explain, and the actual diagnostic recovery loop | Preserve executable commands, expected states, and linked examples |
| `docs/reference/cli.md`, `core-library.md`, `migration-tier-map.md`, `mixed-repo.md`, `canvas-protocol.md`, `canvas-parity.md`, `feature-claims.md` | Corresponding paths under `docs/spec/reference/` | User/API contracts, tier mappings, mixed-project behavior, Canvas protocol, and claim boundaries | Whole-file move initially. Update links, generators, and machine consumers before the old paths disappear |
| `docs/reference/core-surface-ledger.md` and machine companions | Propose `docs/spec/reference/`; keep the machine source and projection relationship | The dated review index and its coverage boundaries | #1398; ledger says generated 2026-08-26. It is not a universal green receipt |
| `docs/reference/errors/` | `docs/spec/reference/errors/` | Eighteen generated error pages plus the `README.md` index | Preserve the index and update the diagnostic-page generator. Do not hand-distill generated errors |
| `docs/infra/{trust-root,index-cache,toolchain-channel}.md` | `docs/spec/packaging/` | Publication, trust, cache, and toolchain contracts; key, hosting, DNS, and deployment prerequisites | These are contracts, not completed deployment evidence |

The remaining reference paths are listed in the appendix. Their move is conditional on the same producer/consumer check; this review does not certify their implementation claims.

## B3: working material and research lifecycle

The first approved action should be preservation and status reconciliation, not bulk deletion. The table names concrete knowledge to retain. A HOLD row cannot be removed under a family-level approval.

| Source/family | Proposed action and destination | Retained knowledge | Status, dependency, or uncertainty |
|---|---|---|---|
| `docs/research/domain-foundations-2026-09.md` and associated fixtures/slates | HOLD active campaign; later put ratified contracts in topical specs and reusable results in a foundations research note | Eleven ratified foundation decisions, domain requirements, card mapping, reasoning, and reproducible evidence | e15; #2762–#2815; owner gates settled does not mean runtime qualification finished |
| `docs/research/jet-foundations-and-trust-2026-09-04/` | HOLD all seven reports and payloads | Current build/readiness findings, recommendations, sources, and unresolved proof | Active work; current build failed, no current project/runtime qualification; #2919/#2925/#2926/#2927 and D-TIER-ONEIR1 |
| `docs/research/embedded-freestanding-profile.md` | HOLD; future embedded topic distillation | What the runtime-layer work covers, missing freestanding Prelude, hybrid alternatives, and explicit absence of a ballot | #2046; partial, not a complete freestanding implementation |
| `docs/research/native-nix-evaluator-conformance-2026-08-24.md` | HOLD; retain evaluator-conformance evidence separately from package-index work | Bounded flake projection, unsupported corpus/graph/closure behavior, and evaluator/index distinction | #2162; five criteria open in the source |
| `docs/research/d-simd3-probe.md` | HOLD; preserve probe inputs and distinct proof levels | D-SIMD3=B, foundation lane result, blocked workspace check, and unverified runtime paths | #2261; a lane CHECK OK is not AOT/JIT/interpreter evidence |
| `docs/research/effect-root-structure-consumers-2026-08-20.md` | Candidate future effects/structure topic note; preserve the original until status is reconciled | Consumer map, absence of new parsed fields, stale BuildEffect-table finding, and absence of a new ballot | #2047; do not invent a decision or treat the defect as fixed |
| `docs/research/compiler-latency-video-crosswalk-2026-08-24.md` | Candidate build-latency research distillation, not deletion of the baseline | Historical comparison, why its baseline was retained, and remaining gaps | #676; stale baseline is intentional evidence, not accidental waste |
| `docs/proposals/domain-matrix.md` and its linked historical research | First concrete superseded-design distillation candidate | Why the 2026-09-02 replacement happened; withdrawn/deleted ballot and card lineage; nine retained substrate cards; transition to domain foundations | Preserve reasons and identities in the decision index and one domain-foundations history note. Recover the complete old source by Git reference. No removal before that extraction is reviewed |
| `docs/plans/docs-cleanup-sweep.md` | Candidate concise cleanup-history entry plus current references in Tower | Accepted 2026-07-25 cleanup choices and why yielding-loops remained active | Accepted is not proof that every linked feature is complete; #1848 must survive |
| `docs/proposals/yielding-loops.md` | HOLD while reconciling current implementation; ratified rules ultimately belong in their topical spec | D-ARROW-CONTROL1, D-LOOPEVAL1, D-LOOPSTATE1, D-COMPREHENSION1 and their exact semantics | Source says ratified 2026-07-26; the cleanup plan explicitly retained it as active |
| `docs/plans/epoch-7/{README,native-jetos,jetos-studio}.md` | Propose a coherent `docs/proposals/jetos/` group, preserving full content initially | Shipped first runtime slice versus future Studio work; native decisions; module/source/diff/proof architecture and long exit criteria | #363/#235; D-JOS-* and D-WD7/8/12. These are mixed and future plans, not finished summaries |
| `docs/continuity/card-1415-script-to-system.md` | HOLD; transfer live work state to #1415 and retain unique design in its topic | Resume-critical state, design constraints, and evidence links | Full status/preservation reconciliation still required; no filename-based completion inference |
| `docs/sidequests/web-backend-wasm.md` | HOLD; unresolved design to proposals, current law to spec when mapped | The partial web backend and open acceptance requirements | #705 remains open in the source |
| `docs/sidequests/generic-modules.md` | HOLD; preserve the ratified choices and remaining work separately | D-GENMOD decisions and actual implementation gaps | Source remains open despite ratified choices |
| `docs/sidequests/library-reuse-and-linking.md` | HOLD; later split current native-library contract from unresolved package-object reuse | Implemented native Library behavior versus unbuilt sealed-package reuse | Ratification is not proof of the remaining implementation |
| `docs/design/self-hosted-optimizer-ssa.md` | `docs/proposals/self-hosted-optimizer-ssa.md`, with all references migrated | Landed minimal TIR boundary versus later static-single-assignment optimizer design | #2301 landed; source still says design |
| Remaining `docs/research/`, `docs/plans/`, `docs/sidequests/`, and `docs/archive/` paths | HOLD for source-specific completion and preservation checks | Unique findings, rejected alternatives, constraints, evidence, unresolved questions | Listed in the appendix. No assumption that old, archived, or agent-written means disposable |

Useful distillations are organized by topic, not by the old file list. Proposed first topics are domain-foundations history, build-latency evidence, embedded readiness, evaluator conformance, and effects/structure lessons. These are proposed destinations, not claims that the distillations have already been written.

An existing `docs/archive/` is not a reason to create another permanent archive. Drain it only through the same reviewed preservation process. Audit and final-report protection still applies to files found there.

## B4: agent-policy preservation map

The problem is conflicting authority as well as length. `AGENTS.md` has 397 lines, owner guidance 63, orchestration 347, and agent memory 2,249. Agent memory calls itself universally read while the main manual treats it as task-triggered. Its historical instructions still tell readers to hand-edit board data and use `git add -A`.

| Current source/content | Proposed maintained destination | Preserve | Retire from ordinary reading |
|---|---|---|---|
| `AGENTS.md` and `docs/agents/owner-guidance.md`: authority, safety, conduct, ownership, model routing | Root `AGENTS.md`, roughly 1,000–1,500 words, without a hard safety-cutting limit | Authority order; I1–I9; owner gates; current capabilities/model policy; ownership; essential command/resource rules; links to exact procedures | Duplicate rule statements and incident narratives |
| `docs/agents/orchestration.md`: dispatch, briefs, shared ownership, integration, proof cadence, recovery | `.agents/skills/orchestration/SKILL.md`, with the existing Tower burndown skill remaining a thin board adapter | Distinct procedures and required proof/closure semantics | The standalone orchestration document after its consumers migrate |
| `docs/agents/agent-memory.md`: current owner policy | The corresponding section of `AGENTS.md` | Current explicit preferences not already represented there | Historical policy overrides and conflicting imperatives |
| Agent memory: reusable technical facts and traps | Relevant topical spec, contributor note, or research distillation | Cause, symptom, fix, applicability, and evidence | Repeated session narratives and obsolete commands |
| Agent memory: current work and blockers | Owning Tower cards | Accurate work state and outstanding proof | A second competing work ledger |
| `docs/agents/owner-guidance-evidence.md` | Precise Git references, plus any unique current reasoning in the relevant maintained destination | Provenance that explains a surviving rule | The archive file only after approved preservation |
| Agent prompts and procedures elsewhere in `docs/agents/` | The relevant skill or `docs/spec/contributing/` | Task-specific operating knowledge | Duplicate global conduct copied into each prompt |

Current authoritative source wins over old memory. The cleanup must not average conflicting instructions. Proposed behavioral changes get explicit review; removing an already-superseded directive does not make that directive current again.

The earlier `D-AGENT-SKILL-CONSOLIDATION1` decision on #2401 remains immutable history. This owner's confirmed design changes its proposed file ownership: root AGENTS becomes the single owner-edited policy file, and orchestration becomes a skill. Domain audit distinctions remain protected. Existing drift/flag checks and all source-path references must follow the approved cutover, rather than being bypassed or left asserting the old layout.

The editor work on #2929 is separate. It points Tower at the existing root file with conflict protection. It does not merge owner guidance or rewrite policy before B4 is approved.

## B5: protect reports and classify proposals by meaning

| Source | Disposition | Knowledge/authority preserved |
|---|---|---|
| `docs/audits/**` | PROTECTED. No agent removal. Approved moves may group reports and their assets | Findings, evidence, negative results, author judgment, and review history |
| `docs/proposals/dogfood-jet-experience-5-of-5.md` | PROTECTED source audit report; propose moving it with its links into audits, not distilling it away | #2386–#2394 and extension findings. Its present directory does not change what it is |
| `docs/reference/dev-dx-benchmark.md` | PROTECTED evidence report; propose audits as its home | Dated 2026-08-27 measurements, peers, conditions, and unfavorable latency results. #2234 remains open |
| Generated HTML companions, screenshots, mockups, and visual-acceptance media | HOLD with their report or active visual work | The rendered evidence and interactions a text summary cannot reproduce |
| `docs/proposals/syntax-lexical-space.md` | Keep unresolved proposal | D-RAWSTR1, D-TRAILCOMMA1, D-SEMI1, D-MARKERSHAPE1, D-MARKERARGS1; the source says nothing is implemented |
| `docs/proposals/automatic-build-optimization.md` | Keep unresolved proposal | D-INCR-UNIT1, D-LIB-REUSE1, D-AOT-CRANELIFT1, D-BUILD-DEFAULT1 and alternatives; proposal, not implementation |
| `docs/proposals/developer-experience.md` | HOLD active/unknown; preserve its benchmark-authority relationship | No Status line; `docs/benchmarks/dx/manifest.json` cites it as authority |
| `docs/proposals/hardening-rig.md` | HOLD incomplete work | D-HARDENING-GATE1=A and machinery #2285–#2287 do not fill the incomplete denominator table |
| `docs/proposals/{records-one-receipt,rights-one-row,shapes-one-fact,claims-one-ladder,open-tables}.md` | Keep unresolved proposals until current source and Tower outcomes are reconciled | Each source explicitly says nothing is implemented; preserve its options and exact claims |

A document with no status remains unknown, not complete. A final report stays owner-removable even if some findings have been distilled into a spec. Do not use an approved distillation to bypass that removal authority.

## B6: why benchmarks are in docs, and what must move together

`docs/benchmarks/dx/` contains executable benchmark inputs, not explanatory documentation. `manifest.json` declares `jet.dx.benchmark-manifest.v1` and names #2479, #2481, and #2482. `run.mjs` is a five-line wrapper importing the real runner from `scripts/agent/dx-benchmark-matrix.mjs`.

Propose `tools/agent-eval/dx/manifest.json` as the maintained manifest home. Keep the existing real runner rather than inventing another harness. Remove the thin docs wrapper only after callers use the canonical runner. This uses an existing functional tool area instead of adding another root directory.

Move the manifest, callers, embedded authority paths, documentation links, and any path-sensitive expectations in one approved change. Known consumers include `scripts/agent/dx-benchmark-matrix.mjs` and `scripts/agent/fresh-agent-domain-run.mjs`. The fresh-agent runner checks manifest context paths and the exact `llms.text` prefix-control contract. Preserve those semantics and the byte budget. Update Tower path references too, especially #2479 criterion 3.

#2481 and #2482 are marked done, but that does not make their executable manifest garbage. #2919 still owns deferred validation. Related compiled-workload proof on #1414 and Epoch 5 closeout on #956 remain open. Source-complete claims cannot retire required proof material.

Other executable research fixtures, scripts, manifests, and machine evidence receive the same treatment: move only with a named functional destination and every consumer. Never summarize an executable fixture or a performance receipt out of existence.

## B7: skill proposals, not a name-count purge

Two independent Tower choices own the proposed changes:

- **#2930 / D-CLEANUP-AUDIT-METHODS1:** keep distinct audit entry points and share genuinely common report mechanics; compare self-contained methods and one entry with complete method profiles.
- **#2931 / D-CLEANUP-WORKFLOW-BOUNDARIES1:** keep independently useful outcome workflows with bounded helpers; compare one planning selector and inline procedural workflows.

Both use the current short profile: one organization choice, three complete worked alternatives, no language syntax or invariant exception. They include current source structure, primary-source comparisons, gains, remaining losses, and reasons against each alternative. Short does not mean a title-only approval request.

The owner ratified A in both ballots. Audit methods keep their actual investigative procedures, not just different labels. A parent workflow owns the requested outcome and every child return. No workflow engine, generated loader, new work ledger, or automatic chain is authorized.

### Distinctions that must survive

| Method | Its own question and evidence | What must not be flattened into a generic audit |
|---|---|---|
| `first-principles-audit` | Re-found one area from primitives, the full corpus, and its overall shape | Independent-reader progression; beginner magic to expert control; same-job before/after; proposal and separately decidable ballot slate |
| `isomorphic-ontology-audit` | Map each Jet form to a language-independent primitive | Ontology mapping, missed equivalences, false visual similarities, clarity, legitimate reuse, and density. Not cosmetic consistency |
| `mission-audit` | Compare Jet with philosophy and I1–I9 | Alignment, drift, and unknown evidence; beginner defaults, expert control, one mechanism, hidden rustc, diagnostics, and useful libraries |
| `pragmatism-audit` | Can people finish representative work without unnecessary ceremony? | At least six workloads; classified friction; useful default, explicit rejection, expert override. Preserve its draft status |
| `surface-audit` | Is the live language and tool surface coherent? | Actual syntax, APIs, defaults, errors, ergonomics, and tooling shape. Do not substitute philosophy scoring |
| `spec-compliance-audit` | How does implementation compare with ratified claims? | Shipped, partial, gap, gated, declined, and stale states; parser/sema/tests/examples and binary probes. Never reopen syntax by accident |
| `type-unification-audit` | Where is compiler knowledge an honest type that users cannot name? | Phantom types, split fact mechanisms, unnameable handles, closed tables; boundary, soundness, adversarial, and up-front design passes. Preserve draft status |
| `surface-frequency-audit` | Which programming work is common in a measured public-code sample, and where does Jet add friction? | Sample boundaries, strata, beginner/general/expert factors, exact surfaces, and recommendation classes. It is report-only, with no default Tower writes |
| `persona-audit` | Can a fresh user in a named domain complete the real core loop? | Push/pull factors; ready, usable-with-friction, blocked, not-proven, and not-applicable distinctions. Embedded card IDs must be checked, not treated as live state |
| `lessons-learned` | Which peer-language successes and regrets should change Jet's choices? | Failure → Jet risk → guard; strength → Jet mechanism; the do-not-ballot list |
| `gauntlet` | Does Jet beat matched peers on real, correct programs? | Paired inputs, output correctness, same-run measurements, per-cell failures, and owning-card gates. Current law allows only Rust parity at ≤1.05; non-Rust ratios must be <1.00 |
| `mine-for-jet` | What does a specific external source teach Jet? | Captured argument, audience differences, claim ledger, linked-source verification, micro-level surface details, comparative probes, and cards/ballots where authorized |
| `surface-research` | What do peer languages offer for a particular Jet surface gap? | Targeted surface ideas, competitive limits, and stopping conditions, rather than a general resource-mining campaign |
| `research` | What do high-trust sources establish about a bounded question? | One findings document with source evidence. Do not silently add an audit lens, mining ledger, or Tower mutations |

Similar subject matter does not make these duplicates. The shared candidate is report plumbing: scope, provenance, evidence qualification, finding shape, and authorized disposition. Empirical sampling, independent-reader design, comparative execution, and specialized completion rules stay with their methods.

A shared procedure must respect output authority. For example, a report-only frequency study may record a finding without creating a Tower card. Common wording must not erase that boundary.

### Other workflow families

The skill research inspected 42 project skills, six Tower plugin skills, and the two existing shared audit references. These are filesystem counts, not a claim that every host loads every file.

The existing common mechanism is `.agents/skills/_shared/standing-lens.md` plus `_shared/audit-dispositions.md`. Reuse them. Do not create a third common protocol beside them.

| Skills | Retain the distinction | Cleanup boundary |
|---|---|---|
| `grilling`, `batch-grill-me`, `grill-with-docs` | One question at a time; one frontier round at a time; interview plus domain record | Cadence and output differ. Preserve both interview methods and honor the requested cadence |
| `wayfinder`, `to-spec`, `to-tickets`, `triage` | Resolve design decisions; synthesize a spec; slice implementation tasks; prepare incoming work | Do not merge decision tickets with implementation tickets or start an interview during context-only synthesis |
| `codebase-design`, `domain-modeling`, `improve-codebase-architecture` | Shared module vocabulary; maintained domain terms and decisions; an actual architecture improvement | Reference material and an end-to-end workflow can share terms without being duplicates |
| `diagnosing-bugs`, `tdd`, `implement`, `resolving-merge-conflicts`, `prototype` | Diagnose a failure; develop test-first; implement a bounded slice; preserve intent during a merge; answer a design question experimentally | Preserve the distinct proof and stopping point. An optional architecture follow-on cannot expand a completed bug fix silently |
| `code-review`, `verify` | Independent Standards/Spec assessment versus machine proof and closeout | Neither replaces the other, and neither creates an extra per-card gate contrary to current policy |
| `eli5`, `eli5-caveman`, `rli5`, `teach` | Explain to a beginner; explain with compression; read as a beginner; build a contextual lesson | RLI5 is not a shorter ELI5. Keep input, reader role, and output distinct |
| `structure-cleanup`, `garbage-collection` | Reorganize without changing behavior versus remove dead or stale material | Semantic changes leave the structure-only workflow. Protected report removal remains owner-only in this campaign |
| `html`, `simple`, `writing-great-skills` | Produce a requested HTML artifact; apply a prose discipline; consult skill-authoring guidance | Do not auto-convert outputs. A useful reference-only skill is not broken merely because it has no ordered workflow |
| `handoff`, `jet-router` | Transfer bounded resume context versus choose the relevant primary workflow | Neither becomes another durable work ledger. Routing chooses a task; it does not execute every linked skill |
| Tower core, ballot, prep, rank, burndown, setup | Board operations; owner choices; work preparation; ordering; delivery; setup | Keep thin adapters and current boundaries. Move general orchestration procedures to the proposed canonical skill, not into six copies |

### Concrete contract repairs proposed

| Source | Observed mismatch | Proposed repair and boundary |
|---|---|---|
| `.agents/skills/mine-for-jet/SKILL.md:39` | Names nonexistent `.agents/skills/tower/SKILL.md` and `tower-ballot/SKILL.md` | Use the actual `plugins/tower/skills/` paths. Preserve mining's distinct evidence method |
| `.agents/skills/to-spec/SKILL.md` | Metadata says synthesize without interviewing, while the procedure asks to check the user's expectations about module boundaries | Choose one explicit intake contract. Synthesize from sufficient context; identify missing owner-gated information rather than starting an unrequested interview |
| `plugins/tower/skills/{tower,tower-prep,tower-ballot}/SKILL.md` | Core/prep retain owner-explicit short rules; prep retains the old six-pass full process. The ballot skill and current store use the newer process | Follow D-BALLOT-PROCESS1=C and current acceptance terms. Short is the default for a one-mechanism choice with up to three options; full uses fresh beginner and rival-family adversarial readers. Do not rewrite historical ballots |
| `.agents/skills/html/SKILL.md` and router/owner routes | Broad automatic triggers and raw Codex execution conflict with explicit-only HTML routing and OMP-first conduct | Reconcile activation and the actual authorized adapter. Do not silently modify a shared installation or invent an exception from an old command |
| `.agents/skills/simple/SKILL.md` and router | Auto-apply text conflicts with opt-in routing | State one current activation rule; keep style references distinct from whole workflows |
| `.agents/skills/grill-with-docs/SKILL.md`, `wayfinder/SKILL.md`, and router | Composition is explicit in skill bodies but its permission and return boundary are not stated consistently | Make the owner's bounded-composition rule explicit. Treat a named composite request as its declared scope, not permission for unrelated follow-ons |
| `.agents/skills/writing-great-skills/SKILL.md` | Invocation guidance makes host-specific reachability and loading claims | Distinguish the documented file format from observed host behavior. Test the active host before claiming metadata hides, blocks, or authorizes a skill |
| Tower setup workflow | Setup instructions include starting the server; the owner alone may do that | Keep server startup behind the explicit owner boundary. Agent setup must stop at the owner-run prerequisite |
| `.agents/skills/research/SKILL.md` | Promises a repository Markdown artifact without a sufficiently explicit destination contract | State the approved `docs/research/` or `docs/audits/` outcome and publishing route, without adding mining or card-writing duties |
| Mine's capture paths and shared write defaults | Stale `/tmp` capture examples and unconditional write expectations can conflict with current resource or read-only task constraints | Use the authorized disk-backed scratch home. Respect a read-only assignment; that constraint does not turn mining and generic research into the same method |

These are source-contract findings, not evidence that a particular harness ignored a rule at runtime. The existing skill index and relevant consolidation and report-disposition checks must agree with approved changes. No new recurring cleanup check is proposed.

### Reference graph is not an execution trace

Tower's core, ballot, prep, rank, and burndown skills link back to each other. That is a cyclic *reference graph*, not proof that an agent recursively executes all those workflows. The cleanup must distinguish navigation, required reading, shared vocabulary, and actual delegated procedures.

Known composed paths include `grill-with-docs → grilling + domain-modeling`, `wayfinder → research/prototype/grilling`, and Tower's preparation/burndown adapters. Keep bounded calls that serve a distinct requested result. Remove a cycle only when it is an execution or ownership problem, not merely because two documents cite each other.

The [Agent Skills specification](https://agentskills.io/specification) defines descriptions, instructions, references, scripts, and assets. It recommends shallow references. [Anthropic's published skill-creator](https://github.com/anthropics/skills/blob/main/skills/skill-creator/SKILL.md) demonstrates one parent outcome with grader, analyst, and schema resources. Neither source proves Jet's host precedence or performance.

All edits remain Jet-local. Inspect shared/global installations to understand discovery and collisions; do not modify them as part of this cleanup.

## Maintained-language repository references

These comparisons support clear separation of purposes, not copying another compiler's folder count.

- [Rust compiler documentation](https://github.com/rust-lang/rust/tree/master/src/doc) separates documentation families. [Rust RFCs](https://github.com/rust-lang/rfcs) retain proposal reasoning and outcomes separately from implementation.
- [Go's specification](https://github.com/golang/go/blob/master/doc/go_spec.html) has a named source. [Go proposals](https://github.com/golang/proposal) preserve design material; [the proposal process](https://go.dev/s/proposal-process) records accepted, declined, and held outcomes.
- [Zig's canonical repository](https://codeberg.org/ziglang/zig) distinguishes documentation and semantic tests. Its [official migration notice](https://ziglang.org/news/migrating-from-github-to-codeberg/) matters when using older GitHub links. No dedicated proposal archive was established by this inspection.
- [Swift's root](https://github.com/swiftlang/swift/tree/main) separates benchmarks, documentation, tests, tools, and user documentation. [Swift Evolution's commonly proposed changes](https://github.com/swiftlang/swift-evolution/blob/main/commonly_proposed.md) demonstrates an accessible record of rejected ideas and their reasons.

Directory presence was inspected through primary repository pages. These observations do not establish popularity, comparative maintenance quality, or unobserved generated-output policies.

## Approval and execution gates

- **B1:** approve only the listed root paths. Recheck content identity, emptiness where applicable, and active ownership before removal. No wildcard purge is included.
- **B2/B6:** approve the proposed homes, then complete the producer/consumer move together. Functional inputs cannot be removed to satisfy the four-folder layout.
- **B3/B4:** the preservation tables define the extraction work. Review the actual distillation or consolidated rule text before source removal. HOLD rows are not deletion candidates.
- **B5:** approve organization separately from removal. Only the owner removes audits and final reports, including this review.
- **B7:** both A verdicts are implemented. Cards #2930 and #2931 are done, with preservation maps, source checks, and bounded cold-reader evidence recorded below. This does not authorize the B4 policy merge or any protected report removal.

### Delivered: root AGENTS editor

#2929 is done. Tower's AGENTS.md tab reads and edits the existing repository-root file. Each save includes its read revision. A stale save is rejected without replacing newer disk text. Generic Docs cannot write arbitrary root files.

The focused persistence and HTTP suites passed all 27 tests after the three new AGENTS cases failed against the old implementation. Eight Chromium scenarios passed on the actual UI and production file helpers with disposable policy files. Desktop (1440×1000) and narrow (390×844) screenshots were inspected.

The browser exercise covered revision conflicts, network failure, retained drafts across Tower navigation, Ctrl/Cmd+S, Escape, focus restoration, newer typing during a save acknowledgement, and completion after leaving the editor. It also covered empty text, read retries, and rejection of the outdated API target. No page errors were observed.

A final read-only check against the existing owner server displayed root AGENTS.md and a revision matching its SHA-256. Both policy files retained their pre-change hashes. No second Tower server was started. The dedicated Chromium process and disposable probes were removed.

Evidence receipt: `/home/nate/.cache/jet-luna/cleanup-agents-browser-proof.json`. The plugin README documents save, conflict, draft-lifetime, cancellation, and file-safety behavior. This editor upgrade does not merge policy content; B4 remains gated.

### Delivered: B7 skill contracts

Both A verdicts are implemented in the shared checkout, and cards #2930 and #2931 are confirmed done. The maps below record the integrated result, not merely the proposal. B4 policy consolidation remains separate.

#### Audit preservation map

Paths in this table are relative to `.agents/skills/`. **Shared reporting** means `_shared/audit-dispositions.md`, specifically its publication, completion, and finding-disposition sections. Its `audit-dispositions:v1` marker and three-column table remain unchanged. `_shared/standing-lens.md` still owns evidence requirements, not a second workflow.

| Source | Maintained destination | Distinct knowledge retained locally |
|---|---|---|
| `_shared/audit-dispositions.md` | Existing file; shared reporting and bounded-work rules | Exact ledger schema; report-only and Tower-owning methods stay distinct |
| `_shared/standing-lens.md` | Existing evidence reference; reporting points to shared reporting | Four questions, five quantities, micro sweep, live probes, and honesty rules |
| `surface-audit/SKILL.md`: reporting boilerplate | Shared reporting; method stays in its existing file | Surface question, category sweep, outlier taxonomy |
| `spec-compliance-audit/SKILL.md`: reporting boilerplate | Shared reporting; method stays local | Ratified-law comparison, six statuses, live evidence, false-shipped traps |
| `mission-audit/SKILL.md`: reporting boilerplate | Shared reporting; method stays local | Philosophy/invariant scorecard and third-facet quantities |
| `persona-audit/SKILL.md`: reporting boilerplate | Shared reporting; method stays local | Persona loops; separate first-window/first-pixel measures; #820–#825 gates; honest non-results |
| `first-principles-audit/SKILL.md`: closing boilerplate | Shared reporting; proposal and Tower phases stay local | Corpus/silhouette, independent evidence, unifying idea, challenged alternatives, owner decisions |
| `isomorphic-ontology-audit/SKILL.md`: reporting boilerplate | Shared reporting; ontology method and reference stay local | Concept mapping, false rhymes, missed equivalences, calibration, report sections |
| `type-unification-audit/SKILL.md`: reporting boilerplate | Shared reporting; type method stays local | Boundary law, hidden/phantom-type census, probes, three review passes |
| `pragmatism-audit/SKILL.md`: reporting boilerplate | Shared reporting; workload method stays local | Six-domain jobs, useful defaults, explicit overrides, friction taxonomy, calibration |
| `surface-frequency-audit/SKILL.md`: repeated reporting rules | Shared reporting; empirical method and references stay local | Frozen corpus, strata, denominators, deduplication, sensitivity, checkpoints, report-only boundary |
| `gauntlet/SKILL.md`: common publication rules | Shared reporting; performance rules and Tower duties stay local | Run/build modes, matched authorship, output checks, strict peer ratios, uncovered-cell failures |
| `mine-for-jet/SKILL.md`: repeated reporting rules | Shared reporting; capture/claim/Tower phases stay local | Source capture, argument and audience, claim ledger, linked checks, live Jet comparison |
| `surface-research/SKILL.md`: reporting boilerplate | Shared reporting; peer-surface method stays local | Targeted gap, ceiling, Jet version, peer change, failure mode; no ratification |
| `lessons-learned/SKILL.md`: reporting boilerplate | Shared reporting; lineage method stays local | Family coverage; failure→risk→guard and strength→mechanism/status, with equal Avoid/Beat depth |

The common file permits declared, bounded evidence helpers that return to the primary owner. It does not launch follow-on audits or weaken a method's explicit Tower obligations. A report-only method may cite existing IDs without creating work.

Mining now names the canonical plugin Tower paths and task-triggered spec lookup. Its scratch path is disk-backed `target-mine-ID`, not `/tmp`. Pragmatism records proposed method improvements in its report; an audit run cannot edit the skill. Compliance teaches how to locate the relevant ratified section rather than demanding an unspecified spec set.

#### Workflow preservation map

Paths below are relative to `.agents/skills/` unless the row names the Tower plugin. Each active composition now states its outcome, inputs, permitted child result, completion owner, return point, and stopping condition. Passive reading is not another workflow.

| Sources | Maintained destination and change | Preserved boundary |
|---|---|---|
| `JetSkillsRouter.md`, `jet-router/SKILL.md` | Existing router contract and entry maps; one primary outcome with bounded returns | All 13 audit/research routes and independent workflow entries remain; retired rows are history, not aliases |
| `grilling/SKILL.md`, `batch-grill-me/SKILL.md` | Existing contracts and interview procedures | One-question and frontier-round interviews remain distinct |
| `grill-with-docs/SKILL.md`, `domain-modeling/SKILL.md`, `triage/SKILL.md` | Existing contracts and caller return points | Requested interview cadence; bounded facts; agreed glossary/ADRs; triage states, labels, verification, and briefs |
| `wayfinder/SKILL.md` | Existing contract, map, and session loop | Decision map, fog/frontier, blocking edges, research/prototype/task distinctions, one decision per session |
| `to-tickets/SKILL.md` | Existing contract, slicing rules, and wide-change section | Settled inputs, tracer bullets, complete vertical slices, blocking edges; one integration-owned clean cutover |
| `to-spec/SKILL.md` | Existing contract, process, and template | Synthesize supplied context; retain seams, verification, exclusions, and publication; unresolved choices remain explicit |
| `research/SKILL.md` | Existing contract and delegation section | Primary sources and one cited result; a caller owns its output boundary, without an automatic second artifact |
| `codebase-design/SKILL.md`, `codebase-design/DESIGN-IT-TWICE.md` | Existing vocabulary and alternative-design handoff | Different constrained alternatives return to one comparison owner; no implementation |
| `improve-codebase-architecture/SKILL.md` | Existing contract, candidate report, and selected-candidate loop | Hot spots, deletion test, HTML cards/diagrams, ADR handling, requested interview cadence |
| `writing-great-skills/SKILL.md`, `writing-great-skills/GLOSSARY.md` | Existing host/composition contract and invocation vocabulary | Progressive disclosure, pruning, and quality checks; metadata no longer claims host execution |
| `html/SKILL.md`, `simple/SKILL.md` | Existing generation/style contracts | HTML visual/accessibility and browser-proof duties; technical-token preservation; style stays a passive transform |
| `plugins/tower/skills/{tower,tower-ballot,tower-prep,tower-setup,tower-burndown,tower-rank}/SKILL.md` | Existing adapter contracts and loops | CLI-backed state, claims, closure barrier, current ballot process, bounded sibling returns, and owner-only server startup |

No workflow engine, replacement audit framework, or shared/global skill copy was added. HTML now uses the authorized OMP route rather than a raw Codex/Sol-first prescription. Tower adapters follow D-BALLOT-PROCESS1=C without rewriting historical ballots.

Two old prescriptions were corrected rather than preserved as behavior. `to-spec` no longer starts an unsolicited seam interview. `to-tickets` no longer recommends expand–contract compatibility batches; its integrated source requires one coherent cutover, with retained old/new forms allowed only by an owner exception. The worker's older preservation-map sentence about retaining expand–contract is superseded by this source-grounded correction.

#### What the exercises proved

**Method routing and resource reachability.** A fresh reader selected the expected primary for all 13 supplied requests by reading the current router. It kept surface, compliance, mission, pragmatism, persona, foundations, ontology, types, frequency, competitive, peer-surface, peer-lessons, and resource-mining work distinct.

The reader also handled a frequency-to-one-resource change, a report-only uncovered finding, strict performance gates, and a bounded first-principles helper. It preserved non-Rust loss at a ratio of 1.00 or higher, Rust's 1.05 parity ceiling, and failure for invalid or unavailable required evidence.

The first read found stale mining references and unclear spec/draft boundaries. Those source defects were fixed. A bounded delta read then opened both exact Tower skill paths and confirmed the task-triggered spec lookup and no-self-edit rule. The original observations and correction receipt are both retained.

**Composition replay with real artifacts.** One fresh worker replayed an already accepted interview from supplied local data. `grill-with-docs` owned the result, honored the requested `batch-grill-me` cadence, accepted a bounded research result, and recorded the agreed domain through `domain-modeling`. It wrote cited facts, a glossary, and a decision record that retained loan history instead of deleting a returned loan. It rejected a helper's suggested mission audit and implementation-ticket agenda.

The replay wrote `docs/research/asset-facts.md`, `CONTEXT.md`, and `docs/adr/0001-retain-loan-history.md` inside an isolated fixture. Their complete contents are retained in the proof receipt before fixture removal. No Tower state or repository domain document was changed by the replay.

**Limits and remaining reading friction.** This was not a live interview, an audit campaign, or automatic host discovery/child invocation. The reader used supplied accepted answers, explicit local inputs, and direct skill reads. The peer-surface routing case omitted its gap; the resource-mining case omitted its URI/path/kind. Both routed correctly, but neither established enough input for a real investigation.

The workflow reader needed to combine the caller's `researchOutput` with the research contract. Domain modeling intentionally leaves full invariant placement to the project context; the fixture used its glossary and ADR. It did not invent a missing definition of Borrower. These are recorded limits of the exercise, not claims of an autonomous end-to-end host run.

#### Focused validation and write boundary

The existing skill check passed in default and `--cold-exercise` modes: 74 disposition rows, 70 filesystem-backed rows, 67 checked Jet names, and a generated drift lock over 128 skill inputs. Its optional model exercised six disposable roots and two guarded writes. That model is not host preflight, and it is not the workflow replay above.

The published review passed the focused disposition check: one report, 19 finding rows, and zero ledger errors. The contract check also confirmed that all nine named audit skills require the shared table.

The checker initially rejected valid current prose because it pinned obsolete orchestration and research sentences. Those assertions were removed instead of changing policy to satisfy old wording. Output now states the source/fixture limits. The audit-disposition check says “ledger errors,” not “unresolved findings,” and the stale Rust assertion pinning the old success sentence was removed.

Supporting consumers changed only in `scripts/agent/check-skill-consolidation.mjs`, `scripts/agent/check-audit-dispositions.mjs`, and `tests/truthfulness.rs`. No compiler build or Rust suite was run for these instruction/checker changes. The actual Node checks and artifact-writing replay supplied their focused proof.

All 42 B7 skill/support paths resolve inside the Jet checkout: 15 audit/reference paths, 24 workflow/router/adapter paths, and three checker consumers. No shared/global installation was a write target. Root policy and the B4 documents were not rewritten; existing protected reports were not changed. This separately authorized review is the only report updated for delivery.

Proof receipt: `/home/nate/.cache/jet-luna/cleanup-skill-proof-2026-09-05.json`. It retains source hashes, both worker preservation maps, the original route read and corrected-resource observation, the workflow trace and full artifacts, and the exact evidence limits.

The disposable workflow and ledger fixtures have been removed. Their required inputs and complete output artifacts remain in the receipt.

Papercut `pc0ri42lw` records the obsolete wording-check failure against #2931. Both checker modes now pass. Tower permits only the owner to resolve a papercut, so that administrative row remains open; it is not an unfixed source failure.

## Appendix: source identities and provisional dispositions

`HEAD` means the captured source matched the recorded baseline. Recover it with `git show 4df728d30bdc4c6317a04898967a39132e4f7538:PATH`. `CHANGED` and `UNTRACKED` mean that command does not recover the captured source; preserve it before removal. A hash identifies content but does not store it.

A family label is not deletion approval. Report protection and active ownership override every proposed reorganization.

### KEEP navigation; update approved links

| Path | Captured state |
|---|---|
| `docs/README.md` | HEAD |

### B4: split by authority, procedure, durable fact, and work state

| Path | Captured state |
|---|---|
| `docs/agents/agent-memory.md` | CHANGED: HOLD |
| `docs/agents/domain.md` | HEAD |
| `docs/agents/examples.md` | HEAD |
| `docs/agents/issue-tracker.md` | HEAD |
| `docs/agents/orchestration.md` | CHANGED: HOLD |
| `docs/agents/owner-guidance-evidence.md` | HEAD |
| `docs/agents/owner-guidance.md` | CHANGED: HOLD |
| `docs/agents/prompts/hardening-rig.md` | HEAD |
| `docs/agents/prompts/whole-language-audit.md` | HEAD |
| `docs/agents/prompts/win-strategy.md` | HEAD |
| `docs/agents/security-closure.md` | HEAD |
| `docs/agents/triage-labels.md` | HEAD |

### B3/B5: no blanket purge; preserve knowledge and protected reports

| Path | Captured state |
|---|---|
| `docs/archive/2026-07-24-logan-smith-rust-series-mining.md` | HEAD |
| `docs/archive/2026-07-24-rust-struct-impl-colocation.md` | HEAD |
| `docs/archive/2026-07-24-verse-video-mining.md` | HEAD |
| `docs/archive/README.md` | HEAD |
| `docs/archive/field-audit-2026-07-23.md` | HEAD |
| `docs/archive/language-lessons-and-regrets.md` | HEAD |
| `docs/archive/language-shape-research.md` | HEAD |
| `docs/archive/lessons-learned-2026-07-23.md` | HEAD |
| `docs/archive/marker-plane-source-of-truth-matrix-2026-07-07.md` | HEAD |
| `docs/archive/mission-audit-2026-07-23.md` | HEAD |
| `docs/archive/package-ecosystem-frameworks.md` | HEAD |
| `docs/archive/persona-audit-2026-07-23.md` | HEAD |
| `docs/archive/ponytail-audit.md` | HEAD |
| `docs/archive/spec-compliance-audit-2026-07-22.md` | HEAD |
| `docs/archive/surface-audit-2026-07-23.md` | HEAD |
| `docs/archive/surface-research-2026-07-23.md` | HEAD |
| `docs/archive/syntax-law-source-status-matrix-2026-07-07.md` | HEAD |

### PROTECTED report; owner removal only

| Path | Captured state |
|---|---|
| `docs/audits/README.md` | HEAD |
| `docs/audits/agent-workload-corpus.md` | HEAD |
| `docs/audits/beginner-onboarding-2026-08-24.md` | HEAD |
| `docs/audits/cli-diagnostics-accessibility.md` | HEAD |
| `docs/audits/compiled-workload-gate.md` | HEAD |
| `docs/audits/compiler-tiers-explained-2026-09-03.html` | UNTRACKED: HOLD |
| `docs/audits/datetime-and-tier-hardening-2026-08-28.md` | HEAD |
| `docs/audits/devtools-dogfood-2026-09-03.md` | UNTRACKED: HOLD |
| `docs/audits/dogfood-jetpack-2026-08-28.md` | HEAD |
| `docs/audits/dogfood-jetpack-usage-experience-2026-08-30.html` | HEAD |
| `docs/audits/dogfood-jetpack-usage-experience-2026-08-30.md` | HEAD |
| `docs/audits/dogfood-tower-2026-08-29.md` | HEAD |
| `docs/audits/effect-authority-census-2026-09-03.md` | UNTRACKED: HOLD |
| `docs/audits/env-as-code-vs-declarative-2026-08-24.md` | HEAD |
| `docs/audits/example-corpus-modernization-2026-08-30.md` | HEAD |
| `docs/audits/field-audit-2026-07-31.md` | HEAD |
| `docs/audits/field-audit-python-2026-07-26.md` | HEAD |
| `docs/audits/fresh-agent-5-of-5-rerun-2026-08-30.md` | HEAD |
| `docs/audits/fresh-agent-5-of-5-rerun-2026-08-31.md` | HEAD |
| `docs/audits/friction-audit-2026-08-30.html` | HEAD |
| `docs/audits/friction-audit-2026-08-30.md` | HEAD |
| `docs/audits/gauntlet-2026-08-27.html` | HEAD |
| `docs/audits/gauntlet-2026-08-27.md` | HEAD |
| `docs/audits/gauntlet-2026-08-28.html` | HEAD |
| `docs/audits/jet-nix-eval-snix-tvix-research-2026-08-24.md` | HEAD |
| `docs/audits/jetpack-native-nixpkgs-2026-08-24.md` | HEAD |
| `docs/audits/optional-args-stdlib-2026-08-28.html` | HEAD |
| `docs/audits/persona-audit-2026-08-14.md` | HEAD |
| `docs/audits/process-session-compatibility-closeout-2026-08-23.md` | HEAD |
| `docs/audits/security-closure-2026-09-02.md` | HEAD |
| `docs/audits/security-deep-scan-2026-08-03-full-tower-control-plane.md` | HEAD |
| `docs/audits/security-deep-scan-2026-08-03-full.md` | HEAD |
| `docs/audits/security-deep-scan-2026-08-03.md` | HEAD |
| `docs/audits/silent-data-sweep-2026-08-28.md` | HEAD |
| `docs/audits/snix-tvix-license-research-2026-08-24.md` | HEAD |
| `docs/audits/syntax-roundtrip-census-2026-09-03.md` | UNTRACKED: HOLD |
| `docs/audits/tier-census-2026-09-03.md` | UNTRACKED: HOLD |
| `docs/audits/tier-parity-architecture-2026-09-03.html` | UNTRACKED: HOLD |
| `docs/audits/tier-parity-architecture-2026-09-03.md` | UNTRACKED: HOLD |
| `docs/audits/video-mine-cpp-loops-simd-build-2026-09-03.html` | UNTRACKED: HOLD |
| `docs/audits/video-mine-cpp-loops-simd-build-2026-09-03.md` | UNTRACKED: HOLD |
| `docs/audits/video-mine-five-languages-2026-08-28.md` | HEAD |
| `docs/audits/video-mine-never-operators-supply-chain-2026-09-01.md` | HEAD |

### B3: preserve design; transfer work state to Tower; HOLD open work

| Path | Captured state |
|---|---|
| `docs/continuity/card-1415-script-to-system.md` | HEAD |
| `docs/plans/README.md` | HEAD |
| `docs/plans/compiler-speed.md` | HEAD |
| `docs/plans/docs-cleanup-sweep.md` | HEAD |
| `docs/plans/epoch-3/README.md` | HEAD |
| `docs/plans/epoch-3/universal-language-core.md` | HEAD |
| `docs/plans/epoch-4/README.md` | HEAD |
| `docs/plans/epoch-4/truth-matrix.md` | HEAD |
| `docs/plans/epoch-4/vision.md` | HEAD |
| `docs/plans/epoch-4/world-class-package-manager.md` | HEAD |
| `docs/plans/epoch-5/README.md` | HEAD |
| `docs/plans/epoch-5/metaprogramming.md` | HEAD |
| `docs/plans/epoch-6/canvas-blueprint-epoch.md` | HEAD |
| `docs/plans/epoch-6/canvas-blueprint-parity-matrix.md` | HEAD |
| `docs/plans/epoch-6/canvas-design-spec.md` | HEAD |
| `docs/plans/epoch-6/canvas-workspace-architecture.md` | HEAD |
| `docs/plans/epoch-7/README.md` | HEAD |
| `docs/plans/epoch-7/jetos-studio.md` | HEAD |
| `docs/plans/epoch-7/native-jetos.md` | HEAD |
| `docs/sidequests/README.md` | HEAD |
| `docs/sidequests/generic-modules.md` | HEAD |
| `docs/sidequests/library-reuse-and-linking.md` | HEAD |
| `docs/sidequests/web-backend-wasm.md` | CHANGED: HOLD |

### B3: unresolved design to proposals; ratified contract to spec

| Path | Captured state |
|---|---|
| `docs/design/self-hosted-optimizer-ssa.md` | HEAD |

### B2: propose spec/guides/; preserve user steps

| Path | Captured state |
|---|---|
| `docs/diagnostic-recovery.md` | HEAD |
| `docs/first-hour.md` | HEAD |

### B2: propose spec/packaging/; retain infrastructure contracts

| Path | Captured state |
|---|---|
| `docs/infra/index-cache.md` | HEAD |
| `docs/infra/toolchain-channel.md` | HEAD |
| `docs/infra/trust-root.md` | HEAD |

### KEEP unresolved design; resolve status before any distillation

| Path | Captured state |
|---|---|
| `docs/proposals/README.md` | HEAD |
| `docs/proposals/automatic-build-optimization.md` | HEAD |
| `docs/proposals/automatic-build-optimization/mockups/report.html` | HEAD |
| `docs/proposals/automatic-build-optimization/mockups/terminal.html` | HEAD |
| `docs/proposals/claims-one-ladder.html` | HEAD |
| `docs/proposals/claims-one-ladder.md` | HEAD |
| `docs/proposals/developer-experience.md` | HEAD |
| `docs/proposals/dogfood-jet-experience-5-of-5.md` | HEAD |
| `docs/proposals/domain-foundations.html` | HEAD |
| `docs/proposals/domain-matrix.html` | HEAD |
| `docs/proposals/domain-matrix.md` | HEAD |
| `docs/proposals/ecosystem-shape.md` | HEAD |
| `docs/proposals/hardening-rig.md` | HEAD |
| `docs/proposals/open-tables.html` | HEAD |
| `docs/proposals/open-tables.md` | HEAD |
| `docs/proposals/records-one-receipt.html` | HEAD |
| `docs/proposals/records-one-receipt.md` | HEAD |
| `docs/proposals/rights-one-row.html` | HEAD |
| `docs/proposals/rights-one-row.md` | HEAD |
| `docs/proposals/shapes-one-fact.html` | HEAD |
| `docs/proposals/shapes-one-fact.md` | HEAD |
| `docs/proposals/stored-invariant-facts.md` | HEAD |
| `docs/proposals/streamline-one-repo.md` | HEAD |
| `docs/proposals/structure-program-is-a-value.md` | HEAD |
| `docs/proposals/syntax-lexical-space.html` | HEAD |
| `docs/proposals/syntax-lexical-space.md` | HEAD |
| `docs/proposals/tier-live-dev.html` | HEAD |
| `docs/proposals/tier-live-dev.md` | HEAD |
| `docs/proposals/transactional-rollback-regions.md` | HEAD |
| `docs/proposals/verdict-loop.html` | HEAD |
| `docs/proposals/verdict-loop.md` | HEAD |
| `docs/proposals/whole-language-frame.html` | HEAD |
| `docs/proposals/whole-language-frame.md` | HEAD |
| `docs/proposals/yielding-loops.md` | HEAD |

### HOLD active visual work and report assets

| Path | Captured state |
|---|---|
| `docs/proposals/prototypes/ballot-surface.html` | HEAD |
| `docs/proposals/prototypes/devtools-ux/A-dock.html` | HEAD |
| `docs/proposals/prototypes/devtools-ux/B-lens.html` | HEAD |
| `docs/proposals/prototypes/devtools-ux/C-workbench.html` | HEAD |
| `docs/proposals/prototypes/devtools-ux/D-pill-lens-workbench.html` | CHANGED: HOLD |
| `docs/proposals/visual-acceptance/index.html` | UNTRACKED: HOLD |

### B2: propose spec/reference/; substantive preservation review

| Path | Captured state |
|---|---|
| `docs/reference/binary-size.md` | HEAD |
| `docs/reference/canvas-parity.md` | HEAD |
| `docs/reference/canvas-protocol.md` | HEAD |
| `docs/reference/cc-driver.md` | HEAD |
| `docs/reference/cli.md` | HEAD |
| `docs/reference/core-backend-facts.md` | HEAD |
| `docs/reference/core-breadth-audit.md` | HEAD |
| `docs/reference/core-library.md` | CHANGED: HOLD |
| `docs/reference/core-surface-ledger.md` | HEAD |
| `docs/reference/dev-dx-benchmark.md` | HEAD |
| `docs/reference/embedded.md` | HEAD |
| `docs/reference/environment.md` | HEAD |
| `docs/reference/feature-claims.md` | HEAD |
| `docs/reference/foreign-build-hosts.md` | HEAD |
| `docs/reference/foreign-octave.md` | HEAD |
| `docs/reference/framework-transplant-closeout.md` | HEAD |
| `docs/reference/jetpack-epoch5.md` | HEAD |
| `docs/reference/language-shape-conformance.md` | HEAD |
| `docs/reference/maturity-tags.md` | HEAD |
| `docs/reference/migration-tier-map.md` | HEAD |
| `docs/reference/mixed-repo.md` | HEAD |
| `docs/reference/network-policy.md` | HEAD |
| `docs/reference/performance-receipt.md` | HEAD |
| `docs/reference/prior-art.md` | CHANGED: HOLD |
| `docs/reference/prove-hostile-matrix.md` | HEAD |
| `docs/reference/tui-interaction.md` | HEAD |
| `docs/reference/versioning.md` | HEAD |

### B2: generated reference; migrate generator and consumers together

| Path | Captured state |
|---|---|
| `docs/reference/errors/E-WEB-ABI-TYPE.md` | HEAD |
| `docs/reference/errors/E-WEB-CROSS-PARTITION.md` | HEAD |
| `docs/reference/errors/E-WEB-TARGET-BROWSER.md` | HEAD |
| `docs/reference/errors/E0101.md` | HEAD |
| `docs/reference/errors/E0102.md` | HEAD |
| `docs/reference/errors/E0103.md` | HEAD |
| `docs/reference/errors/E0104.md` | HEAD |
| `docs/reference/errors/E0105.md` | HEAD |
| `docs/reference/errors/E0107.md` | HEAD |
| `docs/reference/errors/E0108.md` | HEAD |
| `docs/reference/errors/E0109.md` | CHANGED: HOLD |
| `docs/reference/errors/E0110.md` | HEAD |
| `docs/reference/errors/E0111.md` | HEAD |
| `docs/reference/errors/E0119.md` | HEAD |
| `docs/reference/errors/E0120.md` | HEAD |
| `docs/reference/errors/E0356.md` | HEAD |
| `docs/reference/errors/E0357.md` | HEAD |
| `docs/reference/errors/E0359.md` | HEAD |
| `docs/reference/errors/README.md` | HEAD |

### B3: candidate topic distillation only after completion/content checks

| Path | Captured state |
|---|---|
| `docs/research/2026-07-24-devenv-parity.md` | HEAD |
| `docs/research/agent-codegen-benchmark-2026-08-23.md` | HEAD |
| `docs/research/card-1414-compiled-peer-task-definitions.md` | HEAD |
| `docs/research/compiler-latency-video-crosswalk-2026-08-24.md` | HEAD |
| `docs/research/core-absorption-survey-2026-08.md` | HEAD |
| `docs/research/d-simd3-probe.md` | HEAD |
| `docs/research/deploy-rs-lessons-2026-08-06.md` | HEAD |
| `docs/research/domain-foundations-2026-09.md` | HEAD |
| `docs/research/domain-foundations/code/_defects2/report.md` | HEAD |
| `docs/research/domain-foundations/code/area-ai/BRIEF.md` | HEAD |
| `docs/research/domain-foundations/code/area-ai/probe.md` | HEAD |
| `docs/research/domain-foundations/code/area-backend/BRIEF.md` | HEAD |
| `docs/research/domain-foundations/code/area-backend/probe.md` | HEAD |
| `docs/research/domain-foundations/code/area-cli/BRIEF.md` | HEAD |
| `docs/research/domain-foundations/code/area-cli/probe.md` | HEAD |
| `docs/research/domain-foundations/code/area-data/BRIEF.md` | HEAD |
| `docs/research/domain-foundations/code/area-data/probe.md` | HEAD |
| `docs/research/domain-foundations/code/area-embedded/BRIEF.md` | HEAD |
| `docs/research/domain-foundations/code/area-embedded/probe.md` | HEAD |
| `docs/research/domain-foundations/code/area-games/BRIEF.md` | HEAD |
| `docs/research/domain-foundations/code/area-games/probe.md` | HEAD |
| `docs/research/domain-foundations/code/area-gui/BRIEF.md` | HEAD |
| `docs/research/domain-foundations/code/area-gui/probe.md` | HEAD |
| `docs/research/domain-foundations/code/area-web/BRIEF.md` | HEAD |
| `docs/research/domain-foundations/code/area-web/probe.md` | HEAD |
| `docs/research/domain-foundations/code/prim-arrays/BRIEF.md` | HEAD |
| `docs/research/domain-foundations/code/prim-arrays/probe.md` | HEAD |
| `docs/research/domain-foundations/code/prim-bridges/BRIEF.md` | HEAD |
| `docs/research/domain-foundations/code/prim-bridges/probe.md` | HEAD |
| `docs/research/domain-foundations/code/prim-capabilities/BRIEF.md` | HEAD |
| `docs/research/domain-foundations/code/prim-capabilities/probe.md` | HEAD |
| `docs/research/domain-foundations/code/prim-distributed/BRIEF.md` | HEAD |
| `docs/research/domain-foundations/code/prim-distributed/probe.md` | HEAD |
| `docs/research/domain-foundations/code/prim-dsl/BRIEF.md` | HEAD |
| `docs/research/domain-foundations/code/prim-dsl/probe.md` | HEAD |
| `docs/research/domain-foundations/code/prim-exact-numerics/BRIEF.md` | HEAD |
| `docs/research/domain-foundations/code/prim-exact-numerics/probe.md` | HEAD |
| `docs/research/domain-foundations/code/prim-numerics-perf/BRIEF.md` | HEAD |
| `docs/research/domain-foundations/code/prim-numerics-perf/probe.md` | HEAD |
| `docs/research/domain-foundations/code/prim-realtime/BRIEF.md` | HEAD |
| `docs/research/domain-foundations/code/prim-realtime/probe.md` | HEAD |
| `docs/research/domain-foundations/code/prim-receipts/BRIEF.md` | HEAD |
| `docs/research/domain-foundations/code/prim-receipts/probe.md` | HEAD |
| `docs/research/domain-foundations/code/prim-storage/BRIEF.md` | HEAD |
| `docs/research/domain-foundations/code/prim-storage/probe.md` | HEAD |
| `docs/research/domain-foundations/code/prim-text/BRIEF.md` | HEAD |
| `docs/research/domain-foundations/code/prim-text/probe.md` | HEAD |
| `docs/research/domain-foundations/code/prim-time/BRIEF.md` | HEAD |
| `docs/research/domain-foundations/code/prim-time/probe.md` | HEAD |
| `docs/research/domain-foundations/code/prim-tooling-hooks/BRIEF.md` | HEAD |
| `docs/research/domain-foundations/code/prim-tooling-hooks/probe.md` | HEAD |
| `docs/research/domain-foundations/code/prim-units/BRIEF.md` | HEAD |
| `docs/research/domain-foundations/code/prim-units/probe.md` | HEAD |
| `docs/research/domain-foundations/law/BRIEF.md` | HEAD |
| `docs/research/domain-foundations/law/capabilities.md` | HEAD |
| `docs/research/domain-foundations/law/embedded.md` | HEAD |
| `docs/research/domain-foundations/law/ffi.md` | HEAD |
| `docs/research/domain-foundations/law/gui.md` | HEAD |
| `docs/research/domain-foundations/law/http.md` | HEAD |
| `docs/research/domain-foundations/law/lifecycle.md` | HEAD |
| `docs/research/domain-foundations/law/realtime.md` | HEAD |
| `docs/research/domain-foundations/law/receipts.md` | HEAD |
| `docs/research/domain-foundations/law/tensor.md` | HEAD |
| `docs/research/domain-foundations/law/time.md` | HEAD |
| `docs/research/domain-foundations/law/toolseams.md` | HEAD |
| `docs/research/domain-foundations/law/typelevel.md` | HEAD |
| `docs/research/domain-foundations/law/views.md` | HEAD |
| `docs/research/domain-foundations/probes/COMMON.md` | HEAD |
| `docs/research/domain-foundations/probes/area-ai.md` | HEAD |
| `docs/research/domain-foundations/probes/area-backend.md` | HEAD |
| `docs/research/domain-foundations/probes/area-cli.md` | HEAD |
| `docs/research/domain-foundations/probes/area-data.md` | HEAD |
| `docs/research/domain-foundations/probes/area-embedded.md` | HEAD |
| `docs/research/domain-foundations/probes/area-games.md` | HEAD |
| `docs/research/domain-foundations/probes/area-gui.md` | HEAD |
| `docs/research/domain-foundations/probes/area-web.md` | HEAD |
| `docs/research/domain-foundations/probes/prim-arrays.md` | HEAD |
| `docs/research/domain-foundations/probes/prim-bridges.md` | HEAD |
| `docs/research/domain-foundations/probes/prim-capabilities.md` | HEAD |
| `docs/research/domain-foundations/probes/prim-distributed.md` | HEAD |
| `docs/research/domain-foundations/probes/prim-dsl.md` | HEAD |
| `docs/research/domain-foundations/probes/prim-exact-numerics.md` | HEAD |
| `docs/research/domain-foundations/probes/prim-numerics-perf.md` | HEAD |
| `docs/research/domain-foundations/probes/prim-realtime.md` | HEAD |
| `docs/research/domain-foundations/probes/prim-receipts.md` | HEAD |
| `docs/research/domain-foundations/probes/prim-storage.md` | HEAD |
| `docs/research/domain-foundations/probes/prim-text.md` | HEAD |
| `docs/research/domain-foundations/probes/prim-time.md` | HEAD |
| `docs/research/domain-foundations/probes/prim-tooling-hooks.md` | HEAD |
| `docs/research/domain-foundations/probes/prim-units.md` | HEAD |
| `docs/research/domain-foundations/results/area-ai.md` | HEAD |
| `docs/research/domain-foundations/results/area-backend.md` | HEAD |
| `docs/research/domain-foundations/results/area-cli.md` | HEAD |
| `docs/research/domain-foundations/results/area-data.md` | HEAD |
| `docs/research/domain-foundations/results/area-embedded.md` | HEAD |
| `docs/research/domain-foundations/results/area-games.md` | HEAD |
| `docs/research/domain-foundations/results/area-gui.md` | HEAD |
| `docs/research/domain-foundations/results/area-web.md` | HEAD |
| `docs/research/domain-foundations/results/prim-arrays.md` | HEAD |
| `docs/research/domain-foundations/results/prim-bridges.md` | HEAD |
| `docs/research/domain-foundations/results/prim-capabilities.md` | HEAD |
| `docs/research/domain-foundations/results/prim-distributed.md` | HEAD |
| `docs/research/domain-foundations/results/prim-dsl.md` | HEAD |
| `docs/research/domain-foundations/results/prim-exact-numerics.md` | HEAD |
| `docs/research/domain-foundations/results/prim-numerics-perf.md` | HEAD |
| `docs/research/domain-foundations/results/prim-realtime.md` | HEAD |
| `docs/research/domain-foundations/results/prim-receipts.md` | HEAD |
| `docs/research/domain-foundations/results/prim-storage.md` | HEAD |
| `docs/research/domain-foundations/results/prim-text.md` | HEAD |
| `docs/research/domain-foundations/results/prim-time.md` | HEAD |
| `docs/research/domain-foundations/results/prim-tooling-hooks.md` | HEAD |
| `docs/research/domain-foundations/results/prim-units.md` | HEAD |
| `docs/research/domain-matrix-archive/README.md` | HEAD |
| `docs/research/domain-matrix-archive/SUMMARY.md` | HEAD |
| `docs/research/domain-matrix-archive/families/ai-ml.md` | HEAD |
| `docs/research/domain-matrix-archive/families/earth-space.md` | HEAD |
| `docs/research/domain-matrix-archive/families/embedded-hardware.md` | HEAD |
| `docs/research/domain-matrix-archive/families/engineering.md` | HEAD |
| `docs/research/domain-matrix-archive/families/finance-business.md` | HEAD |
| `docs/research/domain-matrix-archive/families/life-sciences-health.md` | HEAD |
| `docs/research/domain-matrix-archive/families/media-creative.md` | HEAD |
| `docs/research/domain-matrix-archive/families/platforms-infrastructure.md` | HEAD |
| `docs/research/domain-matrix-archive/families/science-numerics.md` | HEAD |
| `docs/research/domain-matrix-archive/families/security.md` | HEAD |
| `docs/research/domain-matrix-archive/families/tools-productivity.md` | HEAD |
| `docs/research/dx/domain-registry.md` | HEAD |
| `docs/research/effect-root-structure-consumers-2026-08-20.md` | HEAD |
| `docs/research/embedded-freestanding-profile.md` | HEAD |
| `docs/research/monetization-strategy-2026-08-06.md` | HEAD |
| `docs/research/native-nix-evaluator-conformance-2026-08-24.md` | HEAD |

### HOLD active foundations research

| Path | Captured state |
|---|---|
| `docs/research/jet-foundations-and-trust-2026-09-04/01-foundations.md` | UNTRACKED: HOLD |
| `docs/research/jet-foundations-and-trust-2026-09-04/02-frontier.md` | UNTRACKED: HOLD |
| `docs/research/jet-foundations-and-trust-2026-09-04/03-comprehension.md` | UNTRACKED: HOLD |
| `docs/research/jet-foundations-and-trust-2026-09-04/04-trust.md` | UNTRACKED: HOLD |
| `docs/research/jet-foundations-and-trust-2026-09-04/05-correctness.md` | UNTRACKED: HOLD |
| `docs/research/jet-foundations-and-trust-2026-09-04/06-interop.md` | UNTRACKED: HOLD |
| `docs/research/jet-foundations-and-trust-2026-09-04/07-readiness.md` | UNTRACKED: HOLD |

### KEEP current authority; section-level consolidation only

| Path | Captured state |
|---|---|
| `docs/spec/architecture.md` | HEAD |
| `docs/spec/diagnostic-rows.md` | CHANGED: HOLD |
| `docs/spec/diagnostics.md` | CHANGED: HOLD |
| `docs/spec/encoding-decisions.md` | HEAD |
| `docs/spec/formal-core.md` | HEAD |
| `docs/spec/observability.md` | CHANGED: HOLD |
| `docs/spec/performance-budget-decisions.md` | HEAD |
| `docs/spec/philosophy.md` | HEAD |
| `docs/spec/proof-replay-decisions.md` | HEAD |
| `docs/spec/registry-tiers.md` | HEAD |
| `docs/spec/release-policy.md` | HEAD |
| `docs/spec/roadmap.md` | HEAD |
| `docs/spec/safety.md` | HEAD |
| `docs/spec/spec.md` | CHANGED: HOLD |
| `docs/spec/stdlib-api-laws.md` | HEAD |
| `docs/spec/syntax-decisions.md` | CHANGED: HOLD |
| `docs/spec/tir.md` | HEAD |
| `docs/spec/ui-case-law.md` | HEAD |
| `docs/spec/vocabulary.md` | HEAD |

### Changed and untracked content fingerprints

SHA-256 values identify the inventory snapshot. Before an approved cleanup executes, compare the actual source and preserve any changed version. These fingerprints are not permission to overwrite another task.

| Path | SHA-256 |
|---|---|
| `docs/agents/agent-memory.md` | `2cb35e043b98bd6a8263f75b325f63b2d953f9e94ae9f677f4b9fcf9e675714d` |
| `docs/agents/orchestration.md` | `89ffebf0bed6e55047bdd54570678c949d1d51984529681b1064a8764ea425aa` |
| `docs/agents/owner-guidance.md` | `f90cb7fe2f168ad6ebdea2529cc73d679418a843806fdc22987b85ff19f8f4f3` |
| `docs/proposals/prototypes/devtools-ux/D-pill-lens-workbench.html` | `d74df8e0b0a83ccb4f46c26674eb3b915e647d27df11a0dabd67eaf93961f928` |
| `docs/reference/core-library.md` | `77ff6b602d1d5fb1480267d284c4bc67db07704567f46b37553600d31c1ea55f` |
| `docs/reference/errors/E0109.md` | `60209bfd570845937cbd01e46678ab9444ce663385427e101db620f099f4bd3a` |
| `docs/reference/prior-art.md` | `c865fb5a816367a482e1f4ad22bccb388dee2e2d2de601750ef7ab410925ec0b` |
| `docs/sidequests/web-backend-wasm.md` | `6babdfbe1151e2aa7b8f52a259b86aa9411734279aa556bf05f2f8dbc23461bf` |
| `docs/spec/diagnostic-rows.md` | `6f6391ca0018a02c4f16afa4b6f119ab69ae5f52706935783972aa92de4e8c31` |
| `docs/spec/diagnostics.md` | `c120a98bfa6018b6f1ae325c5bb2e6e74b64d38dab565941d1c0967282eb09e6` |
| `docs/spec/observability.md` | `082fce914d598cff964cfa811aac1d435284998c7050f1eb197b9e56f4589018` |
| `docs/spec/spec.md` | `634dfd1f8d442575dc529920c3e836e3c4f256d823a04bad818c78f2751326c5` |
| `docs/spec/syntax-decisions.md` | `1e5ff14ed4c5d3d0137a8966bbb668a97cc51acbaf285e5c9c1e968e1a12c7ec` |
| `docs/audits/compiler-tiers-explained-2026-09-03.html` | `5f85450c15fb1bc3228e6b8f1ee7d520a5458ffb7e75aa1629db0305cc3fee89` |
| `docs/audits/devtools-dogfood-2026-09-03.md` | `b18669c56a9dd8f7618e7da6e70a63b82359d3715ea02e405c48f27c4ae5fd2c` |
| `docs/audits/effect-authority-census-2026-09-03.md` | `6445074523255c322451b277b1bcdcf30aebfc9b90cee5ca569cebf77cdc4fd8` |
| `docs/audits/syntax-roundtrip-census-2026-09-03.md` | `a4f20fd1add365cf9e2f76dcb48334276228f6749d9a34c26b5170b47c07dd8b` |
| `docs/audits/tier-census-2026-09-03.md` | `ddf1deead02b537ee29a4fe895163db2d0dff2e2f63aff5707666bc58e718900` |
| `docs/audits/tier-parity-architecture-2026-09-03.html` | `d4395229613120066f728963c5e8f53b409cc68dfa74364963128c7a8ebdf371` |
| `docs/audits/tier-parity-architecture-2026-09-03.md` | `bbb14eaa8028abd3e89c4d3f4d04bf5056b8a1fb1099c3aeebd622485f5a9d95` |
| `docs/audits/video-mine-cpp-loops-simd-build-2026-09-03.html` | `0f2d5f1680b951924fde1668b8a202d565b16aea208d2df8047c6a8ddb574240` |
| `docs/audits/video-mine-cpp-loops-simd-build-2026-09-03.md` | `c19b87c00076ab3605c298516159222ed70af87b7802116a36868c0a9868b5d4` |
| `docs/proposals/visual-acceptance/index.html` | `4f9baaac9e0f71340421d5655960d482771e4930ca891d433b9a274db9fa0a31` |
| `docs/research/jet-foundations-and-trust-2026-09-04/01-foundations.md` | `6c409f75d4f7b68af070c29aeef2687fbffdaa6d26d488c18fb45ad1a8ab1389` |
| `docs/research/jet-foundations-and-trust-2026-09-04/02-frontier.md` | `7a27a66a33fbb279bb279a126b508ca80da290464b8a650eadb6ae3e4fea9c72` |
| `docs/research/jet-foundations-and-trust-2026-09-04/03-comprehension.md` | `c5e4386597d8acc050afd97e48ae6a49849a1782d0b79ae3ea92ab775dc897af` |
| `docs/research/jet-foundations-and-trust-2026-09-04/04-trust.md` | `73ca1bd2dea37a55cb3cec583cac5fa7f4c7fa9157ef7234639b754dbac4d87d` |
| `docs/research/jet-foundations-and-trust-2026-09-04/05-correctness.md` | `69ec270a6a7913621f4ac1f21dd42f45e5315dd876551633440ec20ef2649136` |
| `docs/research/jet-foundations-and-trust-2026-09-04/06-interop.md` | `529c668115c6a1af251a16d683892349c513754b9855f14d5db0968d7c6e7264` |
| `docs/research/jet-foundations-and-trust-2026-09-04/07-readiness.md` | `a9e6dd3bfeecaf4e6fcd0d8297d93a75bb744430ec02394191b18b520657ca88` |

## Finding dispositions

These rows record responsibility and approval boundaries, not a claim that pending cleanup has run. A gated `no-action` row means no change is authorized in this run; the finding remains in its batch for review.

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| F1 Root files mistaken for clutter | no-action | B1 exact-path removal approval is still required; retain real inputs and failed evidence. |
| F2 Documentation homes and consumers | no-action | B2 destination and consumer migration approval is still required; no move has run. |
| F3 Research and proposal knowledge at risk | no-action | B3 requires the actual preserved topic/decision text before any source retirement. |
| F4 Repeated agent policy and procedure | no-action | B4 content consolidation remains gated; the editor upgrade did not rewrite policy. |
| F5 Protected report organization | no-action | B5 moves require approval, and only the owner may remove audits or final reports. |
| F6 Executable inputs mixed into docs | no-action | B6 must preserve producers, consumers, fixtures, and evidence before an approved move. |
| F7 Distinct audit methods and repeated reporting | card | #2930 done under D-CLEANUP-AUDIT-METHODS1=A |
| F8 Composed workflow ownership and bounded returns | card | #2931 done under D-CLEANUP-WORKFLOW-BOUNDARIES1=A |
| F9 Mining's obsolete Tower reference paths | card | #2930 |
| F10 Synthesis versus an unrequested interview | card | #2931 |
| F11 Stale Tower ballot preparation rules | card | #2931 under D-BALLOT-PROCESS1=C |
| F12 HTML activation and dispatch mismatch | card | #2931 |
| F13 Style activation mismatch | card | #2931 |
| F14 Unsupported host invocation guarantees | card | #2931 |
| F15 Server startup hidden inside setup | card | #2931 |
| F16 Generic research output destination | card | #2931 |
| F17 Mining scratch and write boundaries | card | #2930 |
| F18 Unversioned save to the old Guidance target | card | #2929 done; focused persistence, HTTP, and browser evidence is recorded above. |
| F19 Static or fixture checks mislabeled as policy/host proof | card | #2931; checker claims and obsolete wording assertions corrected; real reader/replay evidence remains separate. |
<!-- /audit-dispositions -->

## Generalize every finding

This is a one-time source census, not a new recurring audit service. The unprobed instances below are follow-up targets within their owning approved batch, not claims of additional verified failures.

| Finding or risk | Defect shape | Predicted other instances | Structural correction | Owning record |
|---|---|---|---|---|
| `llms.text`, benchmark inputs, and failed receipts resemble clutter | A file's location is mistaken for its function | Research scripts, machine manifests, generated reference pages, test output | Classify producer, consumers, and evidence obligations before removal or relocation | #2928, B1/B2/B6 and the complete path census |
| Ratified or accepted text is mistaken for finished implementation | Design status and qualification status are collapsed | Proposals, dated research, implementation-only closures, old plans | Preserve separate law, implementation, and evidence statements; keep open requirements in Tower | #2928, B3; implementation/proof owners named in the source tables |
| Root policy, owner guidance, orchestration, and memory repeat authority | One rule has competing maintained copies | Model routing, worker checks, proof cadence, retirement instructions in skills | One short AGENTS contract; one canonical orchestration procedure; topical facts and Tower work state | #2928, B4; no source retirement before the preservation diff |
| Similar audit or interview names tempt premature merging | Different questions or methods are mistaken for duplicate steps | ELI5/RLI5, decision planning/task slicing, single-question/frontier interviews | Preserve each question, evidence unit, procedure, and completion boundary; share only genuinely common support | #2930 and #2931 done; both A verdicts implemented with reader/replay evidence |
| Metadata, skill bodies, and routing tables disagree | Invocation contract drifts across representations | Host-specific disable flags, HTML/style triggers, combined workflows | State one route, align its consumers, and test the active host before claiming loader behavior | #2931; Jet-local files only |
| Source wording and a fixture model are treated as runtime proof | A proxy's result is promoted into a stronger claim | Other prose-pinning checks and simulated adapters are candidates, not verified failures | Name the measured property, remove obsolete sentence assertions, and retain separate reader/runtime evidence | #2931; source and fixture results explicitly qualified |
| Tower preparation retains an old ballot process | A historical process remains an active procedure | Other adapters repeating review or authorization rules | Follow the ratified current process and keep historical ballot records unchanged | #2931; D-BALLOT-PROCESS1=C |
| Cross-links look cyclic | A reference graph is mistaken for an execution graph | Shared vocabulary, Tower adapters, explanatory compositions | Distinguish reading/navigation from delegated work; give real calls bounded returns and one completion owner | #2931 |
| Reports live in proposals or reference directories | Directory names are mistaken for removal authority | Archived audits, HTML companions, final research reports | Classify by content; preserve report assets; keep owner-only removal explicit | #2928, B5 |
| The old Guidance save has no content-version precondition | Read-modify-write permits a stale client to replace newer text | Other small document editors are candidates, not verified here | Guard the authorized AGENTS save with its read revision and retain a rejected draft; assess other surfaces separately | #2929 done: 27 focused tests and eight browser scenarios; policy bytes unchanged |
