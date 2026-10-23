import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  END,
  START,
  TARGETS,
  main,
  readActiveMarkers,
  readHighlightTokens,
  renderTreeSitter,
  renderZed,
  replaceSection,
} from "./editor-grammars.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

function fixtureRoot() {
  const root = mkdtempSync(join(tmpdir(), "editor-grammars-"));
  for (const path of ["Compiler/JetFoundation/Data/Syntax", "Compiler/Prelude/Markers.jet", ...TARGETS.map((t) => t.path)]) {
    cpSync(join(ROOT, path), join(root, path), { recursive: true });
  }
  return root;
}

test("committed editor grammar sections match the Jet syntax registry", () => {
  assert.equal(main(["--check"]), 0);
});

test("replaceSection swaps only the marked lines", () => {
  const text = `head\n  // ${START}\nold\n// ${END} trailing\ntail\n`;
  assert.equal(replaceSection(text, `// ${START}\nnew\n// ${END}\n\n`, "x"), `head\n// ${START}\nnew\n// ${END}\ntail\n`);
  assert.throws(() => replaceSection("no markers\n", "", "x.js"), /x\.js has no/);
});

test("retired markers are not highlighted as active rules", () => {
  const root = fixtureRoot();
  try {
    const markers = join(root, "Compiler/Prelude/Markers.jet");
    writeFileSync(markers, `${readFileSync(markers, "utf8")}\nmarker Zzretired($sites: [.Function], $retired: "#Other")\nmarker Zzactive($sites: [.Function])\n`);
    const names = readActiveMarkers(root);
    assert.ok(names.includes("Zzactive"));
    assert.ok(!names.includes("Zzretired"));
    const tokens = readHighlightTokens(root);
    assert.match(renderTreeSitter(tokens), /JET_HIGHLIGHT_MARKER_RULE = \[[^\n]*"Zzactive"/);
    assert.doesNotMatch(renderZed(tokens), /Zzretired/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("--check reports a stale section and --write repairs it", () => {
  const root = fixtureRoot();
  try {
    const grammar = join(root, "Tools/editors/tree-sitter/grammar.js");
    const fresh = readFileSync(grammar, "utf8");
    writeFileSync(grammar, fresh.replace(/const JET_HIGHLIGHT_SIGIL = \[[^\n]*\];/, "const JET_HIGHLIGHT_SIGIL = [];"));
    assert.equal(main(["--check"], root), 1);
    assert.equal(main(["--write"], root), 0);
    assert.equal(readFileSync(grammar, "utf8"), fresh);
    assert.equal(main(["--check"], root), 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("an unresolved highlight constant fails loudly", () => {
  const root = fixtureRoot();
  try {
    const highlights = join(root, "Compiler/JetFoundation/Data/Syntax/Highlights.jet");
    writeFileSync(highlights, readFileSync(highlights, "utf8").replace("text: KW_IF,", "text: KW_NOT_A_CONSTANT,"));
    assert.throws(() => readHighlightTokens(root), /unresolved highlight text constant KW_NOT_A_CONSTANT/);
    assert.equal(main(["--check"], root), 1);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
