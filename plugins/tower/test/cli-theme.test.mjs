import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const TOWER = join(dirname(fileURLToPath(import.meta.url)), '..', 'tower.mjs');
const ANSI = /\x1b\[[0-9;]*m/;
const stripAnsi = (text) => text.replace(/\x1b\[[0-9;]*m/g, '');

function env(extra = {}) {
  const clean = { ...process.env, TOWER_DATA: '' };
  delete clean.NO_COLOR;
  delete clean.FORCE_COLOR;
  return { ...clean, ...extra };
}

function run(cwd, args, extra = {}) {
  return execFileSync(process.execPath, [TOWER, ...args], {
    cwd,
    encoding: 'utf8',
    env: env(extra),
  });
}

function quote(s) {
  return `'${String(s).replaceAll("'", "'\\''")}'`;
}

// util-linux `script` gives the child real stdin/stdout/stderr PTYs without a
// dependency or a fake isTTY flag.
function ptyRun(cwd, args, extra = {}) {
  const command = [process.execPath, TOWER, ...args].map(quote).join(' ');
  return execFileSync('script', ['-q', '-e', '-c', command, '/dev/null'], {
    cwd,
    encoding: 'utf8',
    env: env(extra),
  });
}

function ptyRunAt(cwd, columns, args, extra = {}) {
  const command = `stty cols ${columns}; ${[process.execPath, TOWER, ...args].map(quote).join(' ')}`;
  return execFileSync('script', ['-q', '-e', '-c', command, '/dev/null'], {
    cwd,
    encoding: 'utf8',
    env: env(extra),
  });
}

function fresh() {
  const cwd = mkdtempSync(join(tmpdir(), 'tower-theme-'));
  run(cwd, ['init', '--name', 'Theme']);
  run(cwd, ['card', 'add', '--title', 'Render me', '--priority', 'P1']);
  return cwd;
}

test('human status and brief color resolve flags, env presence, TTY, and redirection', () => {
  const cwd = fresh();

  const status = ptyRun(cwd, ['status', '--color=auto']);
  assert.match(status, ANSI, 'auto colors a real PTY');
  for (const sgr of ['1;96', '2', '32', '33', '31', '7', '90']) {
    assert.ok(status.includes(`\x1b[${sgr}m`), `status uses Theme role SGR ${sgr}`);
  }
  assert.match(ptyRun(cwd, ['brief', '#1', '--color=auto']), ANSI, 'brief shares the Theme');
  assert.match(ptyRun(cwd, ['status', '--color=always']), ANSI, 'always colors a real PTY');
  assert.doesNotMatch(run(cwd, ['status', '--color=auto']), ANSI, 'auto stays clean when redirected');
  assert.match(run(cwd, ['status', '--color=always']), ANSI, 'always overrides redirection');
  assert.doesNotMatch(ptyRun(cwd, ['status', '--color=never']), ANSI, 'never overrides a PTY');

  assert.doesNotMatch(ptyRun(cwd, ['status'], { NO_COLOR: '' }), ANSI, 'empty NO_COLOR still disables color');
  assert.match(run(cwd, ['status'], { FORCE_COLOR: '' }), ANSI, 'empty FORCE_COLOR still enables color');
  assert.doesNotMatch(ptyRun(cwd, ['status'], { NO_COLOR: '', FORCE_COLOR: '' }), ANSI, 'NO_COLOR wins in auto mode');
  assert.match(run(cwd, ['status', '--color=always'], { NO_COLOR: '' }), ANSI, 'explicit always wins over NO_COLOR');
  assert.doesNotMatch(ptyRun(cwd, ['status', '--color=never'], { FORCE_COLOR: '' }), ANSI, 'explicit never wins over FORCE_COLOR');
});

test('JSON status and brief remain byte-clean even when color is forced', () => {
  const cwd = fresh();
  for (const args of [['status', '--json', '--color=always'], ['brief', '#1', '--json', '--color=always']]) {
    const output = run(cwd, args, { FORCE_COLOR: '1' });
    assert.doesNotMatch(output, ANSI);
    assert.doesNotThrow(() => JSON.parse(output));
  }
});

test('human brief prints each open question exactly once', () => {
  const cwd = fresh();
  const text = 'Which terminal should the owner inspect?';
  run(cwd, ['question', 'ask', '#1', '--text', text, '--by', 'owner']);
  const output = run(cwd, ['brief', '#1', '--color=never']);
  assert.equal(output.split(text).length - 1, 1);
});

test('status card rows fit real 40 and 120 column PTYs without losing readable titles', () => {
  const cwd = fresh();
  run(cwd, ['card', 'update', '#1', '--title', 'Render me with a deliberately long title that remains readable while terminal widths change', '--by', 'owner']);
  run(cwd, ['card', 'claim', '#1', '--by', 'width-reviewer']);

  const rowAt = (columns) => {
    const output = ptyRunAt(cwd, columns, ['status', '--color=auto']);
    assert.match(output, ANSI);
    const row = output.split(/\r?\n/).find(line => stripAnsi(line).includes('#1'));
    assert.ok(row, `card row is present at ${columns} columns`);
    return stripAnsi(row);
  };
  const narrow = rowAt(40);
  const wide = rowAt(120);
  assert.ok([...narrow].length <= 40, `40-column row is ${[...narrow].length} columns: ${narrow}`);
  assert.ok([...wide].length <= 120, `120-column row is ${[...wide].length} columns: ${wide}`);
  assert.match(narrow, /Render me with.*…/, 'narrow output keeps a useful title prefix');
  assert.match(wide, /deliberately long title that remains readable/, 'wide output uses available columns');
  assert.match(wide, /\[width-reviewer\]/, 'wide output retains claim context');
});

test('help documents status and brief color controls', () => {
  const output = run(fresh(), ['help']);
  assert.match(output, /tower status .*--color=auto\|always\|never/);
  assert.match(output, /tower brief[^\n]*\n\s+\[--color=auto\|always\|never\]/);
});
