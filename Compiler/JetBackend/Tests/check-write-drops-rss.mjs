#!/usr/bin/env node
// Runtime check that a whole-root write of an owned value releases the value it
// overwrites (Lower.jet lower_release_write_drops / lower_release_slot_deaths).
// Each shape is built natively at two round counts; every round rewrites the
// place 20000 times, so a leaked old value grows the peak RSS by about 1 MB per
// round. The static LIR check cannot see these leaks: an inline List is a stack
// aggregate whose buffer pointer only lives in memory.
//
// usage: node check-write-drops-rss.mjs <jet> <work dir>
// Exits 1 when any shape's peak RSS grows by more than 8 MB from 25 to 200 rounds.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const [jet, work] = process.argv.slice(2);
if (!jet || !work) { console.error('usage: node check-write-drops-rss.mjs <jet> <work dir>'); process.exit(64); }

const shapes = {
  // A list local rebound to a literal inside a loop.
  literal: `touched := [Int]{0, 1}
    loop v in 0..<count {
        touched = [Int]{v, v + 1}
        total += touched.len()
    }`,
  // A list local rebound and then pushed to (a place method borrows it).
  pushed: `touched := [Int]{}
    loop v in 0..<count {
        touched = [Int]{}
        &touched.push(v)
        total += touched.len()
    }`,
  // Another local moved into the outer local.
  moved: `current := [Int]{}
    loop v in 0..<count {
        rest := [Int]{}
        &rest.push(v)
        current = rest
    }
    total += current.len()`,
  // An owned list element overwritten.
  element: `rows := [[Int]]{}
    loop _ in 0..<4 -> &rows.push([Int]{})
    loop v in 0..<count {
        incoming := [Int]{}
        &incoming.push(v)
        rows[v % 4] = incoming
    }
    total += rows.len()`,
  // A String local rebound.
  text: `text := "start"
    loop v in 0..<count { text = "value {v}" }
    total += text.len()`,
  // Controls: a loop-local list and a struct field write.
  local: `loop v in 0..<count {
        fresh := [Int]{}
        &fresh.push(v)
        total += fresh.len()
    }`,
  field: `holder := WriteDropsHolder{items: [Int]{}}
    loop v in 0..<count {
        fresh := [Int]{}
        &fresh.push(v)
        holder.items = fresh
    }
    total += holder.items.len()`,
};

const program = (body, rounds) => `use core.files as write_drops_files

struct WriteDropsHolder { items: [Int] }

fn step(count: Int) -> Int {
    total := 0
    ${body}
    total
}

fn run() {
    total := 0
    loop round in 0..<${rounds} { total += step(20000 + round % 3) }
    status :: write_drops_files.read("/proc/self/status") ?? ""
    loop line in status.split("\\n").to_list() {
        if line.starts_with("VmHWM:") -> print("{line} {total}")
    }
}
`;

const peak = (name, rounds) => {
  const dir = path.join(work, `${name}-${rounds}`);
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, 'unit.jet'), program(shapes[name], rounds));
  fs.writeFileSync(path.join(dir, 'package.jet'), 'name: "write_drops"\nversion: "0.0.1"\nedition: "2028"\nauthority: {\n    holds: { allow: [FS, FS.Read, IO, Mem.Alloc] }\n}\n');
  execFileSync(jet, ['build', 'unit.jet'], { cwd: dir, stdio: ['ignore', 'ignore', 'inherit'] });
  const out = execFileSync(path.join(dir, '.jet/build/unit'), { cwd: dir, encoding: 'utf8' });
  const kb = Number(out.match(/VmHWM:\s+(\d+) kB/)?.[1]);
  if (!Number.isFinite(kb)) throw new Error(`${name}: no VmHWM line in ${JSON.stringify(out)}`);
  return kb;
};

let failed = 0;
for (const name of Object.keys(shapes)) {
  const small = peak(name, 25), large = peak(name, 200);
  const grown = large - small;
  const ok = grown <= 8192;
  if (!ok) failed++;
  console.log(`${ok ? 'ok  ' : 'LEAK'}\t${name}\tpeak ${small} kB at 25 rounds, ${large} kB at 200 (+${grown} kB)`);
}
process.exit(failed ? 1 : 0);
