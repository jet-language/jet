import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { keywordRows, readKeywordRows, renderKeywordSpans, replaceKeywordSpans, SCAN_PATH } from "./lexer-keyword-rows.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

test("canonical keyword projection is current and rejects incomplete authority", () => {
  const rows = readKeywordRows(ROOT);
  const source = readFileSync(resolve(ROOT, SCAN_PATH), "utf8");
  const generated = renderKeywordSpans(rows);
  assert.equal(replaceKeywordSpans(source, generated), source);
  assert.doesNotMatch(generated, /slice_text|\.bytes\(|String\.from_bytes|\[U8\]\{/);
  for (const row of rows) {
    assert.equal(generated.split(`// Syntax::${row.constant}\n`).length - 1, 1);
    assert.match(generated, new RegExp(`\\b${row.tag}\\b`));
  }
  assert.throws(() => replaceKeywordSpans(source + source, generated), /exactly one/);
  const mapping = 'LEXER_KEYWORDS :: [LexerKeyword]{\n    LexerKeyword{constant: "KW_FN", kind: "KwFn", tag: "K_FN"},\n}';
  const syntax = 'KW_FN :: "fn"';
  const tokens = "TOKEN_KW_FN :: 57\n";
  const tags = "K_FN :: 57\n";
  const parse = (m = mapping, s = syntax, t = tokens, k = tags) => keywordRows(m, s, t, k);
  assert.equal(parse()[0].spelling, "fn");
  assert.throws(() => parse(mapping.replace("\n}", '\n    invalid row,\n}')), /unparsed keyword row/);
  assert.throws(() => parse(mapping.replace(/    LexerKeyword.*\n/, "")), /incomplete/);
  assert.throws(() => parse(mapping.replace("KwFn", "KwNew")), /unresolved token kind/);
  assert.throws(() => parse(mapping, syntax, tokens + "TOKEN_KW_NEW :: 58\n"), /incomplete/);
  assert.throws(() => parse(mapping.replace("KwFn", "KwFn(String)")), /unparsed/);
  assert.throws(() => parse(mapping, syntax + "\n" + syntax), /ambiguous/);
  assert.throws(() => parse(mapping, "KW_FN :: OTHER"), /unresolved/);
  assert.throws(() => parse(mapping, 'KW_FN :: "fñ"'), /unsupported keyword/);
  assert.throws(() => parse(mapping, syntax, tokens, ""), /unresolved token kind/);
});
