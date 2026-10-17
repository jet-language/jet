#!/usr/bin/env node
// Runtime check that a write of an owned value releases the value it overwrites
// (Lower.jet lower_release_write_drops / lower_release_slot_deaths): every owned
// heap kind rebound inside a loop, then written through the rebound place, and
// nested places (a field of a local, an element or map entry of a field).
// Each shape is built natively at two round counts; every round rewrites the
// place 20000 times, so a leaked old value grows the peak RSS by about 1 MB per
// round. The static LIR check cannot see these leaks: an inline List or Map is a
// stack aggregate whose buffer pointer only lives in memory.
//
// usage: node check-write-drops-rss.mjs <jet> <work dir> [small rounds] [large rounds]
// Exits 1 when any shape fails to build or run, or its peak RSS grows by more
// than 8 MB from the small to the large round count (default 25 and 200).
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const [jet, work, smallArg, largeArg] = process.argv.slice(2);
if (!jet || !work) { console.error('usage: node check-write-drops-rss.mjs <jet> <work dir> [small rounds] [large rounds]'); process.exit(64); }
const smallRounds = Number(smallArg ?? 25), largeRounds = Number(largeArg ?? 200);

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
  // A list local rebound from its own old value (read, then overwritten).
  self: `acc := [Int]{0}
    loop v in 0..<count { acc = [Int]{acc.len() % 7, v} }
    total += acc.len()`,
  // A String local rebound; rebound, then appended to.
  text: `text := "start"
    loop v in 0..<count { text = "value {v}" }
    total += text.len()`,
  text_append: `text := "start"
    loop v in 0..<count {
        text = "value {v}"
        text += "!"
    }
    total += text.len()`,
  // Every other owned heap kind rebound inside a loop, then written through the
  // rebound place: a projected write (a map entry) or a method on the place.
  map_index: `seen := [Int:Bool]{}
    loop v in 0..<count {
        seen = [Int:Bool]{}
        seen[v] = true
    }
    total += seen.len()`,
  map_string: `names := [String:String]{}
    loop v in 0..<count {
        names = [String:String]{}
        names["k{v}"] = "v{v}"
        names["k{v}"] = "w{v}"
    }
    total += names.len()`,
  map_method: `table := [Int:Int]{}
    loop v in 0..<count {
        table = [Int:Int]{}
        &table.add(v, v).drop("fresh map")
    }
    total += table.len()`,
  map_value: `entries := [Int:[Int]]{}
    loop v in 0..<count {
        fresh := [Int]{}
        &fresh.push(v)
        entries[v % 4] = fresh
    }
    total += entries.len()`,
  set: `s := Set.from([0])
    loop v in 0..<count {
        s = Set.from([v])
        &s.add(v + 1)
    }
    total += s.len()`,
  tally: `t := Tally<Int>.new()
    loop v in 0..<count {
        t = Tally<Int>.new()
        &t.add(v)
    }
    total += t.count(1)`,
  queue: `q := Queue.init([0])
    loop v in 0..<count {
        q = Queue.init([v])
        &q.push_back(v + 1)
    }
    total += q.len()`,
  pqueue: `pq := PriorityQueue.from([0])
    loop v in 0..<count {
        pq = PriorityQueue.from([v, 1])
        &pq.push(v + 2)
    }
    total += pq.len()`,
  bits: `bits := Bits.new()
    loop v in 0..<count {
        bits = Bits.new()
        &bits.add(v % 64 + 200)
    }
    total += bits.len()`,
  bytes: `buffer := Bytes.new()
    loop v in 0..<count {
        buffer = Bytes.new()
        &buffer.write_u8(U8{1})
    }
    total += buffer.len()`,
  // Option and enum boxes holding a list.
  option: `best := write_drops_dims(0)
    loop v in 0..<count { best = write_drops_dims(v) }
    if best == .Val(items) -> total += items.len()`,
  enum: `wrap := WriteDropsWrap.Empty
    loop v in 0..<count { wrap = WriteDropsWrap.Full{items: [Int]{v, v}} }
    if wrap == .Full(items) -> total += items.len()`,
  // Structs holding owned values: the whole struct rebound, then a field, a
  // map entry of a field and an element of a field written.
  struct_field: `holder := WriteDropsHolder{items: [Int]{}}
    loop v in 0..<count {
        holder = WriteDropsHolder{items: [Int]{v}}
        fresh := [Int]{}
        &fresh.push(v)
        holder.items = fresh
    }
    total += holder.items.len()`,
  struct_entry: `tables := WriteDropsTables{table: [String:Int]{}, rows: [[Int]]{}}
    loop v in 0..<count {
        tables = WriteDropsTables{table: [String:Int]{}, rows: [[Int]]{[Int]{}}}
        tables.table["k{v}"] = v
        incoming := [Int]{}
        &incoming.push(v)
        tables.rows[0] = incoming
    }
    total += tables.table.len() + tables.rows.len()`,
  field_map: `tables := WriteDropsTables{table: [String:Int]{}, rows: [[Int]]{}}
    loop v in 0..<count {
        tables.table = [String:Int]{}
        tables.table["k{v}"] = v
    }
    total += tables.table.len()`,
  field_element: `tables := WriteDropsTables{table: [String:Int]{}, rows: [[Int]]{[Int]{}, [Int]{}}}
    loop v in 0..<count {
        incoming := [Int]{}
        &incoming.push(v)
        tables.rows[v % 2] = incoming
    }
    total += tables.rows.len()`,
  list_of_maps: `maps := [[Int:Int]]{[Int:Int]{}, [Int:Int]{}}
    loop v in 0..<count {
        fresh := [Int:Int]{}
        fresh[v] = v
        maps[v % 2] = fresh
        maps[v % 2][v + 1] = v
    }
    total += maps.len()`,
  // A closure owning a captured String, rebound.
  closure: `show := (n: Int) -> "start{n}"
    loop v in 0..<count {
        mark :: "t{v}"
        show = (n: Int) -> "{mark}{n}"
        total += show(v).len()
    }`,
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
struct WriteDropsTables { table: [String:Int], rows: [[Int]] }
enum WriteDropsWrap {
    Empty
    Full(items: [Int])
}

fn write_drops_dims(n: Int) -> [Int]? {
    if n % 2 == 0 -> return Val([Int]{n})
    None
}

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
  let small, large;
  try {
    small = peak(name, smallRounds);
    large = peak(name, largeRounds);
  } catch (error) {
    failed++;
    console.log(`FAIL\t${name}\t${String(error.message).split('\n')[0]}`);
    continue;
  }
  const grown = large - small;
  const ok = grown <= 8192;
  if (!ok) failed++;
  console.log(`${ok ? 'ok  ' : 'LEAK'}\t${name}\tpeak ${small} kB at ${smallRounds} rounds, ${large} kB at ${largeRounds} (+${grown} kB)`);
}
process.exit(failed ? 1 : 0);
