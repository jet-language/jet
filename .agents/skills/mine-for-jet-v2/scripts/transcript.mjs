#!/usr/bin/env node
// Caption cleanup only: never factual verification or body-review evidence.
import { readFileSync, writeFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

export function transcript(packet) {
  if (!Array.isArray(packet.events)) throw new Error('expected json3 events array');
  const rows = [];
  for (const event of packet.events) {
    if (!event.segs) continue;
    const value = event.segs.map(segment => segment.utf8 ?? '').join('').replace(/\s+/g, ' ').trim();
    if (!value) continue;
    if (!Number.isFinite(event.tStartMs) || event.tStartMs < 0) throw new Error('caption event has invalid start time');
    const previous = rows.at(-1);
    const nearby = previous && event.tStartMs - previous.last_update_ms >= 0 && event.tStartMs - previous.last_update_ms <= 5000;
    if (nearby && (value === previous.text || value.startsWith(`${previous.text} `))) {
      previous.text = value; previous.last_update_ms = event.tStartMs;
    } else rows.push({ start_ms: event.tStartMs, last_update_ms: event.tStartMs, text: value });
  }
  return rows;
}
export function render(rows) {
  return rows.map(row => {
    const seconds = Math.floor(row.start_ms / 1000);
    return `[${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}] ${row.text}`;
  }).join('\n') + '\n';
}
export function main(argv = process.argv.slice(2)) {
  try {
    let input = null; let output = null;
    for (let i = 0; i < argv.length; i++) {
      const option = argv[i];
      if (!['--input', '--output'].includes(option)) throw new Error(`unknown option ${option}`);
      const value = argv[++i]; if (!value || value.startsWith('--')) throw new Error(`missing value for ${option}`);
      if (option === '--input') input = value; else output = value;
    }
    if (!input || !output) throw new Error('usage: transcript.mjs --input CAPTURE.json3 --output CAPTURE.txt');
    writeFileSync(output, render(transcript(JSON.parse(readFileSync(input, 'utf8')))), { flag: 'wx' });
    return 0;
  } catch (error) { process.stderr.write(`transcript: ${error.message}\n`); return 1; }
}
if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) process.exitCode = main();
