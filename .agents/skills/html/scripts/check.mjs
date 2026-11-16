#!/usr/bin/env node
// Contract check for pages produced by the html skill. Usage: node .agents/skills/html/scripts/check.mjs <file.html> [...]
// Exit 1 when any file fails. Prints one line per check per file.
import fs from 'node:fs';

const RETIRED = [/--ivory\b/, /--clay\b/, /--olive\b/, /--oat\b/, /--sky\b/, /#FAF9F5/i, /#D97757/i, /#788C5D/i, /#B04A3F/i, /#E3DACC/i, /#F0EEE6/i, /#D1CFC5/i, /#87867F/i];

// Density and hierarchy (owner ruling, 2026-09-03): a report is scanned, not read.
// These run on the body only, with markup stripped inside each paragraph.
const body = (s) => { const i = s.search(/<body[\s>]/i); return i < 0 ? s : s.slice(i).replace(/<script[\s\S]*?<\/script>/gi, '').replace(/<style[\s\S]*?<\/style>/gi, ''); };
const words = (html) => html.replace(/<[^>]+>/g, ' ').replace(/&[a-z]+;/g, ' ').trim().split(/\s+/).filter(Boolean).length;
const paragraphs = (s) => [...body(s).matchAll(/<p\b[^>]*>([\s\S]*?)<\/p>/gi)].map((m) => ({ tag: m[0], text: m[1], words: words(m[1]) }));
const isReport = (s) => /class="[^"]*\bsection-head\b/.test(s); // tools have their own shape and skip the density rules
// Loose prose = words inside a <p> with no structural ancestor. A structural ancestor is a list, table, code block, figure,
// or any element whose class names a card, grid, pair, sequence, step, stat, tile, row, item, term, chip, callout, or timeline.
const STRUCTURAL_TAG = /^(pre|table|ol|ul|figure|dl)$/;
const STRUCTURAL_CLASS = /\b(card|grid|pair|sequence|step|stat|tile|row|item|term|chip|callout|timeline)[\w-]*\b/;
const VOID = /^(br|hr|img|input|meta|link|wbr|source)$/;
function sectionProseShares(html) {
  const shares = [];
  const stack = []; // { tag, structural }
  let section = null; // { total, loose }
  let pDepth = 0, pLoose = false;
  const re = /<\/?([a-zA-Z][\w-]*)([^>]*)>|([^<]+)/g;
  let m;
  while ((m = re.exec(html))) {
    if (m[3] !== undefined) {
      if (section) { const n = words(m[3]); section.total += n; if (pDepth && pLoose) section.loose += n; }
      continue;
    }
    const tag = m[1].toLowerCase(), closing = m[0][1] === '/', attrs = m[2] || '';
    if (closing) {
      const i = stack.map((x) => x.tag).lastIndexOf(tag);
      if (i >= 0) stack.splice(i);
      if (tag === 'p' && pDepth) pDepth--;
      if (tag === 'section' && section) { if (section.total >= 120) shares.push({ loose: section.loose, share: section.loose / section.total }); section = null; }
      continue;
    }
    if (VOID.test(tag) || /\/\s*$/.test(attrs)) continue;
    const cls = (attrs.match(/class="([^"]*)"/) || [])[1] || '';
    const structural = STRUCTURAL_TAG.test(tag) || STRUCTURAL_CLASS.test(cls);
    stack.push({ tag, structural });
    if (tag === 'section') section = { total: 0, loose: 0 };
    if (tag === 'p') { pDepth++; pLoose = !stack.some((x) => x.structural); }
  }
  return shares;
}
const paragraphCap = (s) => { if (!isReport(s)) return true; const long = paragraphs(s).filter((p) => p.words > 90); return long.length === 0; };
const proseRun = (s) => {
  if (!isReport(s)) return true;
  // Three consecutive <p> siblings (nothing but whitespace between them) with more than 40 words each is a prose wall.
  const b = body(s);
  const re = /(<p\b[^>]*>[\s\S]*?<\/p>)\s*(<p\b[^>]*>[\s\S]*?<\/p>)\s*(<p\b[^>]*>[\s\S]*?<\/p>)/gi;
  let m; while ((m = re.exec(b))) { if ([m[1], m[2], m[3]].every((p) => words(p) > 40)) return false; re.lastIndex = m.index + m[1].length; }
  return true;
};
// A section is a prose wall when it holds over 150 words of loose paragraphs and they are more than three quarters of its words.
const sectionStructure = (s) => { if (!isReport(s)) return true; return sectionProseShares(body(s)).every((x) => !(x.loose > 150 && x.share > 0.75)); };
const headlineFirst = (s) => { if (!isReport(s)) return true; const acts = /^\s*(where we (were|are|could be)|what (comes next|has to be decided))\s*$/i; return ![...body(s).matchAll(/<h2\b[^>]*>([\s\S]*?)<\/h2>/gi)].some((m) => acts.test(m[1].replace(/<[^>]+>/g, ''))); };

const checks = [
  ['doctype', (s) => /^\s*(<!--[\s\S]*?-->\s*)?<!doctype html>/i.test(s)],
  ['charset', (s) => /<meta charset="utf-8">/i.test(s)],
  ['viewport', (s) => /<meta name="viewport"/i.test(s)],
  ['color-scheme dark', (s) => /<meta name="color-scheme" content="dark">/i.test(s) || /color-scheme:\s*dark/.test(s)],
  ['single style block', (s) => (s.match(/<style[\s>]/gi) || []).length === 1],
  ['no external resources', (s) => !/https?:\/\/|<link\b|@import|src=["']\/\//i.test(s.replace(/<!--[\s\S]*?-->/g, ''))],
  ['jet palette tokens', (s) => ['--jet', '--bone', '--ember', '--oxblood', '--smoke', '--ok', '--amber', '--cyan', '--blue'].every((t) => s.includes(t + ':'))],
  ['code is highlighted', (s) => !/<pre[\s>]/.test(s) || /class="hl-|function hl\(/.test(s)],
  ['code wraps, never scrolls sideways', (s) => /pre\s*\{[^}]*white-space:\s*pre-wrap/.test(s)],
  ['no retired palette', (s) => !RETIRED.some((re) => re.test(s))],
  ['display face is Exo 2 italic', (s) => /--display:\s*"Exo 2"/.test(s)],
  ['fonts embedded, not linked', (s) => /@font-face[\s\S]*?src:\s*url\(data:font\/woff2;base64,/.test(s)],
  ['ignition line', (s) => /class="[^"]*\bignition\b/.test(s) && /@keyframes ignite/.test(s)],
  ['scroll reveal', (s) => /IntersectionObserver/.test(s) && /is-visible/.test(s)],
  ['reduced motion honored', (s) => /prefers-reduced-motion:\s*reduce/.test(s)],
  ['focus visible', (s) => /:focus-visible/.test(s)],
  ['complete document', (s) => /<\/html>\s*$/i.test(s)],
  ['no paragraph over 90 words', paragraphCap],
  ['no prose wall (three long paragraphs in a row)', proseRun],
  ['every section has a structured element', sectionStructure],
  ['headlines state the takeaway, not the act name', headlineFirst],
];

let failed = false;
for (const file of process.argv.slice(2)) {
  const s = fs.readFileSync(file, 'utf8');
  const results = checks.map(([name, fn]) => [name, fn(s)]);
  const bad = results.filter(([, ok]) => !ok);
  console.log(`${bad.length ? 'FAIL' : 'ok  '} ${file}${bad.length ? ' — ' + bad.map(([n]) => n).join(', ') : ''}`);
  if (bad.length) failed = true;
}
process.exit(failed ? 1 : 0);
