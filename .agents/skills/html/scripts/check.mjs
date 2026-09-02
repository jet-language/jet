#!/usr/bin/env node
// Contract check for pages produced by the html skill. Usage: node .agents/skills/html/scripts/check.mjs <file.html> [...]
// Exit 1 when any file fails. Prints one line per check per file.
import fs from 'node:fs';

const RETIRED = [/--ivory\b/, /--clay\b/, /--olive\b/, /--oat\b/, /--sky\b/, /#FAF9F5/i, /#D97757/i, /#788C5D/i, /#B04A3F/i, /#E3DACC/i, /#F0EEE6/i, /#D1CFC5/i, /#87867F/i];
const checks = [
  ['doctype', (s) => /^\s*(<!--[\s\S]*?-->\s*)?<!doctype html>/i.test(s)],
  ['charset', (s) => /<meta charset="utf-8">/i.test(s)],
  ['viewport', (s) => /<meta name="viewport"/i.test(s)],
  ['color-scheme dark', (s) => /<meta name="color-scheme" content="dark">/i.test(s) || /color-scheme:\s*dark/.test(s)],
  ['single style block', (s) => (s.match(/<style[\s>]/gi) || []).length === 1],
  ['no external resources', (s) => !/https?:\/\/|<link\b|@import|src=["']\/\//i.test(s.replace(/<!--[\s\S]*?-->/g, ''))],
  ['jet palette tokens', (s) => ['--jet', '--bone', '--ember', '--oxblood', '--smoke'].every((t) => s.includes(t + ':'))],
  ['no retired palette', (s) => !RETIRED.some((re) => re.test(s))],
  ['display type is monospace', (s) => /--display:\s*ui-monospace/.test(s)],
  ['ignition line', (s) => /class="[^"]*\bignition\b/.test(s) && /@keyframes ignite/.test(s)],
  ['scroll reveal', (s) => /IntersectionObserver/.test(s) && /is-visible/.test(s)],
  ['reduced motion honored', (s) => /prefers-reduced-motion:\s*reduce/.test(s)],
  ['focus visible', (s) => /:focus-visible/.test(s)],
  ['complete document', (s) => /<\/html>\s*$/i.test(s)],
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
