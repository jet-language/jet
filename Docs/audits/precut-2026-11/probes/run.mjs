// Run precut probe programs on the pinned compiler, the way the golden suite runs an example.
// Usage: node probes.mjs JET TREE PROBE_DIR OUT.json [NAME_FILTER]
// Each probe NAME.jet starts with header comments:
//   // cards: 3013 4407          the cards the probe proves
//   // claim: one plain sentence
//   // mode: run | same | check-error CODE | check-clean | test
//   // exit: N                    expected exit code for run and test (default 0)
//   // allow: --allow-all         extra run flags (optional)
// mode run compares stdout of o0 (jet run), o1 (jet build) and o2 (jet build --release) with NAME.out.
// mode same requires the three tiers to agree on stdout, stderr text and exit code.
// mode check-text runs jet check; it needs every `// has:` text and no `// lacks:` text,
// and `// errors: N` pins the count of `Error [` lines.
// `// stderr-has: TEXT` adds a stderr requirement on every tier for run and same.
import { readFileSync, readdirSync, writeFileSync, mkdirSync, existsSync, statSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join, basename } from 'node:path';

const [jet, tree, dir, out, filter] = process.argv.slice(2);
const identity = readFileSync(join(jet, '..', 'IDENTITY'), 'utf8').trim();
// Each run builds into its own fresh directory, so no earlier output is ever removed.
const scratch = join(dir, 'work', `run-${Date.now()}`);
mkdirSync(join(scratch, 'tmp'), { recursive: true });
const env = { ...process.env, JET_TOOLCHAIN_ROOT: tree, JET_COMPILER_IDENTITY: identity, TMPDIR: join(scratch, 'tmp'), NO_COLOR: '1' };
const exec = (argv, cwd) => {
  const r = spawnSync(argv[0], argv.slice(1), { cwd, env, encoding: 'utf8', timeout: 300000, maxBuffer: 1 << 26 });
  return { code: r.status ?? (r.signal ? `signal ${r.signal}` : 'timeout'), stdout: r.stdout ?? '', stderr: (r.stderr ?? '') + (r.error ? `\n${r.error.message}` : '') };
};
const header = (src, key) => (new RegExp(`^// ${key}: (.*)$`, 'm').exec(src)?.[1] ?? '').trim();
const results = [];
for (const file of readdirSync(dir).filter(f => /^[^.].*\.jet$/.test(f)).sort()) {
  const name = basename(file, '.jet');
  if (filter && !name.includes(filter)) continue;
  const path = join(dir, file);
  const src = readFileSync(path, 'utf8');
  const mode = header(src, 'mode') || 'run';
  const allow = header(src, 'allow').split(' ').filter(Boolean);
  const exit = Number(header(src, 'exit') || 0);
  const row = { name, cards: header(src, 'cards').split(/\s+/).filter(Boolean).map(Number), claim: header(src, 'claim'), mode, tiers: {} };
  const [word, code] = mode.split(' ');
  const all = key => [...src.matchAll(new RegExp(`^// ${key}: (.*)$`, 'gm'))].map(m => m[1].trim());
  const stderrHas = all('stderr-has');
  if (word === 'check-error' || word === 'check-clean' || word === 'check-text') {
    const r = exec([jet, 'check', path], dir);
    const text = r.stdout + r.stderr;
    row.tiers.check = { code: r.code, output: text.slice(0, 4000) };
    const errors = header(src, 'errors');
    if (word === 'check-clean') row.pass = r.code === 0 && !/Warning \[|Error \[/.test(text);
    else if (word === 'check-error') row.pass = r.code !== 0 && text.includes(code);
    else row.pass = all('has').every(h => text.includes(h)) && all('lacks').every(h => !text.includes(h)) && (errors === '' || (text.match(/Error \[/g) ?? []).length === Number(errors));
  } else if (word === 'test') {
    const r = exec([jet, 'test', path], dir);
    row.tiers.test = { code: r.code, output: (r.stdout + r.stderr).slice(0, 4000) };
    row.pass = r.code === exit;
  } else {
    const expected = existsSync(join(dir, `${name}.out`)) ? readFileSync(join(dir, `${name}.out`), 'utf8') : null;
    for (const tier of ['o0', 'o1', 'o2']) {
      let r;
      if (tier === 'o0') r = exec([jet, 'run', ...allow, path], dir);
      else {
        const bin = join(scratch, `${name}-${tier}`);
        mkdirSync(bin, { recursive: true });
        const b = exec([jet, 'build', path, `--output-dir=${bin}`, ...(tier === 'o2' ? ['--release'] : [])], dir);
        const made = existsSync(bin) ? readdirSync(bin).map(f => join(bin, f)).filter(f => statSync(f).isFile() && (statSync(f).mode & 0o111)) : [];
        r = b.code !== 0 || !made.length ? { ...b, stderr: `build: ${b.stderr}` } : exec([made[0]], dir);
      }
      row.tiers[tier] = { code: r.code, stdout: r.stdout.slice(0, 4000), stderr: r.stderr.slice(0, 4000) };
    }
    const t = Object.values(row.tiers);
    const streams = t.every(x => !x.stderr.startsWith('build: ') && stderrHas.every(h => x.stderr.includes(h)));
    if (word === 'same') row.pass = streams && t.every(x => x.stdout === t[0].stdout && x.code === t[0].code);
    else row.pass = streams && expected !== null && t.every(x => x.stdout === expected && x.code === exit);
  }
  results.push(row);
  console.log(`${row.pass ? 'PASS' : 'FAIL'}\t${name}\t${row.cards.join(',')}\t${Object.entries(row.tiers).map(([k, v]) => `${k}=${v.code}`).join(' ')}`);
}
writeFileSync(out, JSON.stringify(results, null, 1));
