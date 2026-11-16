// Comment and sigil census over tracked .jet files (read-only).
// An approximate Jet scanner: it skips "...", """...""" (with {…} holes that
// may hold nested strings), `` raw fences `` of any length, char literals 'x',
// and nested /* */ comments.
// usage: [OUT=file.json] node census.mjs <repo> [dirs...]
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';

const repo = process.argv[2];
const dirs = process.argv.slice(3).length ? process.argv.slice(3) : ['Compiler', 'Core', 'Examples', 'tests', 'Jetpack'];
const env = { ...process.env };
delete env.GIT_INDEX_FILE;
const files = execFileSync('git', ['-C', repo, 'ls-files', '-z', '--', ...dirs.map((d) => `${d}/*.jet`), ...dirs.map((d) => `${d}/**/*.jet`)], { env, maxBuffer: 1 << 28 })
  .toString().split('\0').filter(Boolean);
const uniq = [...new Set(files)];

const zero = () => ({ files: 0, lines: 0, lineComment: 0, fullLine: 0, trailing: 0, doc: 0, moduleDoc: 0, quadSlash: 0,
  block: 0, blockLines: 0, nestedBlock: 0, docBlockStar: 0, hashTok: 0, hashHash: 0, shebang: 0, commentHash: 0,
  commentStartsHash: 0, docMdHeading: 0, commentHasHashHash: 0, commentedMarker: 0, commentHasPaste: 0,
  slashOp: 0, slashPercent: 0, slashEq: 0, urlOutsideString: 0, codeUrlLike: 0, emptyComment: 0,
  noSpaceAfter: 0, filesWithComment: 0 });
const per = {};
const total = zero();
const samples = { commentedMarker: [], docMdHeading: [], commentStartsHash: [], urlOutsideString: [], nestedBlock: [], moduleDoc: [], commentHasPaste: [], hashHash: [] };

function scan(src, c, file) {
  const n = src.length;
  let i = 0;
  let line = 1;
  let lineHasCode = false;
  const lineStart = () => src.lastIndexOf('\n', i - 1) + 1;
  let any = false;
  if (src.startsWith('#!')) { c.shebang++; while (i < n && src[i] !== '\n') i++; }
  function skipString() { // at a quote
    const triple = src.startsWith('"""', i);
    i += triple ? 3 : 1;
    while (i < n) {
      const ch = src[i];
      if (triple && src.startsWith('"""', i)) { i += 3; return; }
      if (!triple && ch === '"') { i++; return; }
      if (!triple && ch === '\n') { return; }
      if (ch === '\\') { i += 2; continue; }
      if (ch === '\n') line++;
      if (ch === '{' && src[i + 1] === '{') { i += 2; continue; }
      if (ch === '{') { i++; skipCode('}'); continue; }
      i++;
    }
  }
  function skipCode(close) { // inside an interpolation hole
    let depth = 1;
    while (i < n) {
      const ch = src[i];
      if (ch === '"') { skipString(); continue; }
      if (ch === '{') depth++;
      if (ch === '}') { depth--; if (depth === 0) { i++; return; } }
      if (ch === '\n') { line++; i++; return; }
      i++;
    }
  }
  while (i < n) {
    const ch = src[i];
    if (ch === '\n') { line++; lineHasCode = false; i++; continue; }
    if (ch === ' ' || ch === '\t' || ch === '\r') { i++; continue; }
    if (ch === '"') { lineHasCode = true; skipString(); continue; }
    if (ch === '`') {
      lineHasCode = true;
      let len = 0; while (src[i + len] === '`') len++;
      let j = i + len; let closed = false;
      while (j < n) {
        if (src[j] !== '`') { if (src[j] === '\n') line++; j++; continue; }
        let run = 0; while (src[j + run] === '`') run++;
        j += run;
        if (run === len) { closed = true; break; }
      }
      i = closed ? j : n; continue;
    }
    if (ch === "'" && (src[i + 2] === "'" || (src[i + 1] === '\\' && src.indexOf("'", i + 2) - i <= 12))) {
      lineHasCode = true; const end = src.indexOf("'", i + 2); i = end < 0 ? n : end + 1; continue;
    }
    if (ch === '/' && src[i + 1] === '/') {
      let e = src.indexOf('\n', i); if (e < 0) e = n;
      const text = src.slice(i, e);
      any = true;
      c.lineComment++;
      const isDoc = text.startsWith('///') && !text.startsWith('////');
      if (text.startsWith('////')) c.quadSlash++;
      if (isDoc) c.doc++;
      else if (text.startsWith('//!')) { c.moduleDoc++; if (samples.moduleDoc.length < 5) samples.moduleDoc.push(`${file}:${line}`); }
      else if (lineHasCode) c.trailing++;
      else c.fullLine++;
      const body = text.replace(/^\/\/[\/!]?/, '');
      if (body.trim() === '') c.emptyComment++;
      if (/^[^ \t]/.test(body) && body.length) c.noSpaceAfter++;
      if (body.includes('#')) c.commentHash++;
      if (/^\s*#/.test(body)) { c.commentStartsHash++; if (samples.commentStartsHash.length < 12) samples.commentStartsHash.push(`${file}:${line}: ${text.slice(0, 90)}`); }
      if (isDoc && /^\s*#{1,6}\s/.test(body)) { c.docMdHeading++; if (samples.docMdHeading.length < 8) samples.docMdHeading.push(`${file}:${line}: ${text.slice(0, 90)}`); }
      if (body.includes('##')) c.commentHasHashHash++;
      if (/^\s*#!?\[?[A-Z][A-Za-z.]*/.test(body)) { c.commentedMarker++; if (samples.commentedMarker.length < 12) samples.commentedMarker.push(`${file}:${line}: ${text.slice(0, 90)}`); }
      if (body.includes('#{')) { c.commentHasPaste++; if (samples.commentHasPaste.length < 6) samples.commentHasPaste.push(`${file}:${line}: ${text.slice(0, 90)}`); }
      i = e; continue;
    }
    if (ch === '/' && src[i + 1] === '*') {
      any = true;
      c.block++;
      if (src[i + 2] === '*' && src[i + 3] !== '/') c.docBlockStar++;
      let depth = 1; let j = i + 2; const startLine = line; let nested = false;
      while (j < n && depth > 0) {
        if (src[j] === '/' && src[j + 1] === '*') { depth++; nested = true; j += 2; continue; }
        if (src[j] === '*' && src[j + 1] === '/') { depth--; j += 2; continue; }
        if (src[j] === '\n') line++;
        j++;
      }
      if (nested) { c.nestedBlock++; if (samples.nestedBlock.length < 5) samples.nestedBlock.push(`${file}:${startLine}`); }
      c.blockLines += line - startLine + 1;
      i = j; continue;
    }
    lineHasCode = true;
    if (ch === '#') {
      if (src[i + 1] === '!' && src[i + 2] === '/') { c.shebang++; }
      c.hashTok++;
      if (src[i + 1] === '#') { c.hashHash++; if (samples.hashHash.length < 8) samples.hashHash.push(`${file}:${line}`); }
      i++; continue;
    }
    if (ch === '/') {
      if (src[i + 1] === '%') c.slashPercent++;
      else if (src[i + 1] === '=') c.slashEq++;
      else c.slashOp++;
      i++; continue;
    }
    if (ch === ':' && src[i + 1] === '/' && src[i + 2] === '/') { c.urlOutsideString++; if (samples.urlOutsideString.length < 8) samples.urlOutsideString.push(`${file}:${line}`); }
    i++;
  }
  c.lines += line;
  if (any) c.filesWithComment++;
}

for (const f of uniq) {
  const top = f.split('/')[0];
  per[top] ||= zero();
  let src;
  try { src = readFileSync(`${repo}/${f}`, 'utf8'); } catch { continue; }
  const c = zero();
  scan(src, c, f);
  c.files = 1;
  for (const k of Object.keys(c)) { per[top][k] += c[k]; total[k] += c[k]; }
}
const out = { repo, dirs, per, total, samples };
const json = JSON.stringify(out, null, 1);
if (process.env.OUT) writeFileSync(process.env.OUT, json + '\n');
console.log(json);
