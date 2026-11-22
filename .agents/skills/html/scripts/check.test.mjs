#!/usr/bin/env node
// Tests for .agents/skills/html/scripts/check.mjs:
// - citation hyperlinks (<a href="https://...">) pass while loaded external dependencies fail
// - proseRun allows two separate pairs of long paragraphs in different containers and fails three consecutive sibling long paragraphs
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const dir = path.dirname(fileURLToPath(import.meta.url));
const checkScript = path.join(dir, 'check.mjs');

const longPara = (label) =>
  `<p>${label} ` + Array.from({ length: 45 }, (_, i) => `word${i}`).join(' ') + '</p>';

function makePage({ headExtra = '', styleExtra = '', bodyContent = '' } = {}) {
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="color-scheme" content="dark">
${headExtra}
<style>
@font-face { font-family: "Exo 2"; src: url(data:font/woff2;base64,AAAA); }
:root { --jet:#000; --bone:#fff; --ember:#f60; --oxblood:#600; --smoke:#333; --ok:#0f0; --amber:#fa0; --cyan:#0ff; --blue:#08f; --display: "Exo 2"; }
pre { white-space: pre-wrap; }
@keyframes ignite { from { opacity: 0; } to { opacity: 1; } }
@media (prefers-reduced-motion: reduce) { * { animation: none; } }
:focus-visible { outline: 2px solid var(--cyan); }
.is-visible { opacity: 1; }
${styleExtra}
</style>
</head>
<body>
<div class="ignition"></div>
<div class="section-head"><h2>Concrete findings from live measurements</h2></div>
${bodyContent}
<script>const io = new IntersectionObserver(() => {});</script>
</body>
</html>`;
}

function runCheck(html, name) {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'jet-html-check-'));
  const file = path.join(tmp, `${name}.html`);
  fs.writeFileSync(file, html, 'utf8');
  try {
    const out = execFileSync(process.execPath, [checkScript, file], { encoding: 'utf8' });
    return { code: 0, out };
  } catch (err) {
    return { code: err.status ?? 1, out: (err.stdout || '') + (err.stderr || '') };
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

// 1. Citation hyperlink (<a href="https://...">) passes "no external resources"
{
  const res = runCheck(
    makePage({
      bodyContent: '<section><div class="card"><p>See <a href="https://jet-lang.dev/spec">the Jet specification</a>.</p></div></section>',
    }),
    'citation-link-pass',
  );
  assert.equal(res.code, 0, `expected citation link page to pass, got:\n${res.out}`);
}

// 2. External <script src="https://..."> fails "no external resources"
{
  const res = runCheck(
    makePage({
      headExtra: '<script src="https://cdn.example.com/app.js"></script>',
    }),
    'external-script-fail',
  );
  assert.notEqual(res.code, 0, 'expected external script to fail');
  assert.match(res.out, /no external resources/);
}

// 3. External <link rel="stylesheet" href="https://..."> fails "no external resources"
{
  const res = runCheck(
    makePage({
      headExtra: '<link rel="stylesheet" href="https://cdn.example.com/site.css">',
    }),
    'external-stylesheet-fail',
  );
  assert.notEqual(res.code, 0, 'expected external stylesheet to fail');
  assert.match(res.out, /no external resources/);
}

// 4. CSS @import and <img src="https://..."> and CSS url(https://...) fail "no external resources"
{
  const resImport = runCheck(
    makePage({ styleExtra: '@import url("https://cdn.example.com/base.css");' }),
    'external-import-fail',
  );
  assert.notEqual(resImport.code, 0, 'expected @import to fail');
  assert.match(resImport.out, /no external resources/);

  const resImg = runCheck(
    makePage({ bodyContent: '<img src="https://cdn.example.com/logo.png" alt="x">' }),
    'external-img-fail',
  );
  assert.notEqual(resImg.code, 0, 'expected external img src to fail');
  assert.match(resImg.out, /no external resources/);

  const resFont = runCheck(
    makePage({ styleExtra: '@font-face { font-family: "Bad"; src: url("https://cdn.example.com/f.woff2"); }' }),
    'external-font-fail',
  );
  assert.notEqual(resFont.code, 0, 'expected external font url to fail');
  assert.match(resFont.out, /no external resources/);
}

// 5. Two separate pairs of long paragraphs in different containers pass proseRun
{
  const res = runCheck(
    makePage({
      bodyContent: `<section><div class="card">${longPara('A')}${longPara('B')}</div><div class="card">${longPara('C')}${longPara('D')}</div></section>`,
    }),
    'separate-pairs-pass',
  );
  assert.equal(res.code, 0, `expected separate pairs of long paragraphs to pass, got:\n${res.out}`);
}

// 6. Three consecutive sibling long paragraphs fail proseRun
{
  const res = runCheck(
    makePage({
      bodyContent: `<section><div class="card">${longPara('A')}\n${longPara('B')}\n${longPara('C')}</div></section>`,
    }),
    'three-siblings-fail',
  );
  assert.notEqual(res.code, 0, 'expected three consecutive long sibling paragraphs to fail');
  assert.match(res.out, /no prose wall/);
}

console.log('check.test.mjs: all 6 checks passed');
