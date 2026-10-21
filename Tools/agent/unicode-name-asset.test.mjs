// core.text `unicode_name` reads the generated UNICODE_NAME_INDEX asset
// (kind 8: 11-byte rows [scalar:3, pool offset:4, length:4] + a UTF-8 pool).
// The asset replaced a 162,765-line generated Jet decision tree; this pins it
// to that tree's exact meaning: the same 40,531 (scalar, name) rows, every
// other scalar UNASSIGNED. The hash is of the sorted "scalar\tname\n" rows the
// tree returned (ec9648355 Core/Text/Text.jet, exhaustive leaf parity 0 diffs).
import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

const TREE_ROWS = 40531;
const TREE_SHA256 = "a60dd4cc39c63787a010ddb7a70b20daa37106405784a2912e3583891ea340c0";

function decodeNameAsset() {
  const source = readFileSync(new URL("../../Core/Text/Runtime/Assets/UnicodeNameIndex.jet", import.meta.url), "utf8");
  const literal = source.match(/UNICODE_NAME_INDEX :: \[U8\]\{"([^"]*)"\}/)?.[1];
  assert.ok(literal, "UNICODE_NAME_INDEX byte literal");
  const bytes = Buffer.from(literal.split("\\x").slice(1).map((pair) => parseInt(pair, 16)));
  assert.equal(bytes.toString("latin1", 0, 4), "JUCD");
  assert.equal(bytes.readUInt32LE(12), 8, "kind 8 (UTF-8 names)");
  assert.equal(bytes.readUInt32LE(20), 11, "stride");
  const count = bytes.readUInt32LE(16);
  const records = bytes.readUInt32LE(24);
  const pool = bytes.readUInt32LE(28);
  assert.equal(bytes.readUInt32LE(36), bytes.length, "extent");
  const rows = [];
  for (let row = 0; row < count; row++) {
    const at = records + row * 11;
    const start = bytes.readUInt32LE(at + 3);
    const length = bytes.readUInt32LE(at + 7);
    assert.ok(pool + start + length <= bytes.length, `row ${row} name extent`);
    rows.push([bytes.readUIntLE(at, 3), bytes.toString("utf8", pool + start, pool + start + length)]);
  }
  return rows;
}

test("UNICODE_NAME_INDEX carries the retired decision tree's exact names", () => {
  const rows = decodeNameAsset();
  assert.equal(rows.length, TREE_ROWS);
  for (let index = 1; index < rows.length; index++) assert.ok(rows[index - 1][0] < rows[index][0], `sorted at row ${index}`);
  assert.deepEqual(rows[0], [0, "NULL"]);
  assert.deepEqual(rows[rows.length - 1], [917999, "VARIATION SELECTOR-256"]);
  const digest = createHash("sha256").update(rows.map(([cp, name]) => `${cp}\t${name}\n`).join("")).digest("hex");
  assert.equal(digest, TREE_SHA256);
});
