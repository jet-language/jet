import { execFileSync } from "node:child_process";
const R = "/home/nate/Projects/Github/jet";
const BY = "fable-e3-burndown";
const t = (a) => { try { return { code: 0, out: execFileSync("node", ["plugins/tower/tower.mjs", ...a], { cwd: R, encoding: "utf8" }).trim() }; } catch (e) { return { code: e.status ?? 1, out: ((e.stdout||"")+(e.stderr||"")).trim() }; } };

const STALE = "Premise refuted by measurement at c04e904a6, not implemented as written. ";

const cards = {
  1761: {
    all: STALE + "All five stems this card batches are already resident_jit in the re-derived ledger: errors/partial_and_notes:250, io/db_policy:271, tooling/app_live:388, types/traits:458, ui/ui_component_kit:469. That section is an observed parity proof rather than a label - classify_corpus_stem diverts a stem to TierDivergent unless jit_out == aot_out, separately asserts interpreter == AOT, and requires both that no deopt occurred and that Cranelift actually executed. None of the five appears in tier_divergent or in run_tier_broken, which is empty. The three root causes the card recorded (a ui_component_kit AOT internal compiler error, app_live's LiveQuery lowering, and types/traits printing an unsubstituted Option<T>) were each closed by earlier work this epoch. The card's own cited gap rows are also gone: jit_gaps.txt gaps: and run_gaps: are both empty. Recorded rather than silently closed: the stem list in circulation was itself stale - it named errors/or_err, while the card names errors/partial_and_notes.",
  },
  1758: {
    all: STALE + "The card states coll.list_zip only ever implemented the two-input no-pad case. That is false in the tree: lower_zip_family at crates/jet-jit/src/jit/lower_ctx.rs:11363-11462 implements generic N-ary zip through JitZipPlan and the iter_zip_family host, with modes Short, Strict and Pad and fills DefaultNone, Common and Columns. Every shape in examples/features/collections/zip_family.jet is covered - zip(), 1-ary, .zip, .zip_short, 3-ary, 4-ary, zip_pad, fill: and fills: - and collections/zip_family records resident_jit in the re-derived ledger. One construct still refuses, flatten at lower_ctx.rs:11374-11376, and it is unused by the corpus; the refusal is consistent between the predicate and the lowering, so it is an honest refusal rather than a gap. A sibling divergence in collections/iter_adapters is tracked on #2024 and is not this family: that lane confirmed it owns no zip arm.",
  },
  1756: {
    all: STALE + "There is no blanket core.services deopt. A real 14-arm allowlist sits at crates/jet-jit/src/jit/safety.rs:1606-1666 with full arity dispatch at lower_ctx.rs:12786-12853 and core.sync.text_new at :12833, and all four stems sit in compile_covered with jit_gaps.txt gaps: and run_gaps: both empty. What landed is an I8 dedup rather than new coverage: service_core_arity became pub(crate) and is now the single table consulted by BOTH the lowering and tier admission, replacing a hand-restated allowlist plus a superseded app/core.web sync branch. The two sides were machine-diffed before the deletion - 69 rows each, zero set difference, zero arity disagreement - so admission and lowering now agree structurally instead of coincidentally, with no construct newly admitted. The card's real blocker turned out to be on the AOT side and is recorded separately: tooling/service_authority, service_runtime and service_tree all FAIL AOT compilation, and the gate was absolving them by filing AOT failure under expected_exit. The concrete cause is now known - two Prelude fragments each emit use std::time::{Duration, ...}, so any program pulling in both gets generated Rust that rustc rejects with E0252, which per I2 is an internal compiler error. That defect and the gate-class split are tracked on #2016 and #2026.",
  },
};

for (const [num, spec] of Object.entries(cards)) {
  const list = t(["card", "criteria", "#" + num, "--list"]);
  const n = (list.out.match(/^#\d+ /gm) || []).length;
  for (let i = 1; i <= n; i++) {
    const r = t(["card", "criteria", "#" + num, "--meet", String(i), "--evidence", spec.all, "--by", BY]);
    if (r.code) console.log(`#${num} c${i} SKIP ${r.out.slice(0, 70)}`);
  }
  const d = t(["card", "update", "#" + num, "--phase", "done", "--by", BY]);
  console.log(`#${num} (${n} crit) -> ${d.code === 0 ? "done" : d.out.slice(0, 110)}`);
}
