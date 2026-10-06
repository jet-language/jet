// env-split.mjs COMPILER_DIR [--write]: the mechanical half of the J environment split
// (FRONTEND-300X §3.1). Every function parameter `<name>: [&]SemaScopeEnvironment` gains
// `program: SemaProgramTables` after it (`&SemaProgramTables` in the value-landing writers), every
// call argument `[&]<name>` gains `program` after it (`&program` in a call to a writer), and
// `<name>.<program field>` reads become `program.<field>`. Prints counts and the sites left for
// hand edits (pass drivers that bind or store environments, the SemaScopeEnvironment struct and
// constructor). Without --write it only reports. Rerunnable: an already-split header/call (followed by
// `program`) is left alone, so a second pass makes 0 edits.
import fs from 'node:fs';
import path from 'node:path';

const [root, flag] = process.argv.slice(2);
const write = flag === '--write';
const FIELDS = ['registration_graph', 'checked_types', 'checked_type_rows', 'checked_type_name_slots', 'checked_type_name_rows', 'boxed_edges'];
// Functions that write a program field (value landing); they take the tables by `&`.
const WRITERS = ['jet_driver_comptime_apply_landed'];

const files = [];
(function walk(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(full);
    else if (entry.name.endsWith('.jet')) files.push(full);
  }
})(root);

const counts = { files: 0, headers: 0, calls: 0, reads: 0 };
const manual = [];
for (const file of files) {
  const before = fs.readFileSync(file, 'utf8');
  // One chunk per function (the text before the first function is its own chunk). Calls and reads
  // are rewritten only for the environment name the chunk's header declares, or that the chunk
  // binds from an environment constructor (pass drivers; their `program` binding is a hand edit).
  const starts = [0];
  const fnRe = /^(?:pub )?fn [a-z_0-9]+\(/gm;
  let m;
  while ((m = fnRe.exec(before))) if (m.index > 0) starts.push(m.index);
  const chunks = starts.map((at, i) => before.slice(at, i + 1 < starts.length ? starts[i + 1] : before.length));
  const out = chunks.map((chunk) => {
    const name = (chunk.match(/^(?:pub )?fn ([a-z_0-9]+)\(/) ?? [])[1];
    const names = new Set();
    let text = chunk.replace(/^((?:pub )?fn [a-z_0-9]+\([^]*?\)[^{]*\{)/, (header) =>
      header.replace(/\b([a-z_][a-z_0-9]*): (&?)SemaScopeEnvironment(?!\s*,\s*program:)/g, (p, param) => {
        counts.headers++;
        names.add(param);
        return `${p}, program: ${WRITERS.includes(name) ? '&' : ''}SemaProgramTables`;
      }));
    // Locals bound to an environment (pass drivers' constructors, re-targeted or copied environments)
    // pass the same program tables beside them.
    for (const bound of chunk.matchAll(/\b([a-z_]*environment) :?= /g)) names.add(bound[1]);
    for (const env of names) {
      // A moved environment (`^environment`) goes into a pass driver's shared state, not to a check.
      const call = new RegExp(`([(,]\\s*)(&?)(${env})(\\s*)(?=[,)])(?!\\s*,\\s*[&~^]?program\\b)`, 'g');
      text = text.replace(call, (_c, lead, mark, n, space) => {
        counts.calls++;
        return `${lead}${mark}${n}${space}, program`;
      });
      for (const writer of WRITERS) {
        const writerCall = new RegExp(`(\\b${writer}\\(\\s*&?${env}\\s*,\\s*)program\\b`, 'g');
        text = text.replace(writerCall, (_w, lead) => `${lead}&program`);
      }
      const read = new RegExp(`\\b${env}\\.(${FIELDS.join('|')})\\b`, 'g');
      text = text.replace(read, (_r, field) => {
        counts.reads++;
        return `program.${field}`;
      });
    }
    return text;
  });
  const text = out.join('');
  const lines = text.split('\n');
  lines.forEach((line, i) => {
    if (/\b(environment|pass_environment)\s*:?= |SemaScopeEnvironment\{|\[SemaScopeEnvironment\]|struct SemaScopeEnvironment/.test(line)) {
      manual.push(`${path.relative(root, file)}:${i + 1}: ${line.trim().slice(0, 140)}`);
    }
  });
  if (text !== before) {
    counts.files++;
    if (write) fs.writeFileSync(file, text);
  }
}
console.log(JSON.stringify(counts));
console.log(`manual sites (${manual.length}):`);
for (const site of manual) console.log(site);
