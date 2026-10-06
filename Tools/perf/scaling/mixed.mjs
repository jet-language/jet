// The `mixed` profile of generate.mjs: one deterministic Jet package sized
// by nonblank, noncomment ("code") lines, shaped like the assembled
// self-hosted compiler (Tower #4529, SPEED-PLAN.md section 4.3): about 25
// lines per function and 53 bytes per line, structs, enums with payloads, a
// trait called through trait values, generic functions, closures, value and
// variant matches, maps, strings and interpolation, and about 4.7%
// compile-time (`prep`) items. `fn run` prints one checksum line.
//
// The package is one source file (main.jet) of numbered sections, like the
// assembled compiler unit. Both compilers that must build it disagree on a
// multi-file package today: the Rust reference requires `module part_0` for
// a sibling file and cannot lower a generic instantiated inside one ("missing
// checked function target"), while the self-hosted compiler loads sibling
// files itself and rejects the `module` line (E0105 "defined twice"); it also
// cannot call a trait method through a bounded type parameter ("MIR trait
// receiver is not a trait object"), so trait calls go through trait values.
//
// The generator also evaluates every generated function itself, so the
// expected output is derived from the same template parameters that wrote
// the source; the harness pins it as the golden. All arithmetic stays below
// 2^53 and nonnegative (every combine is reduced modulo MODULUS), so the
// JavaScript model and Jet's Int agree exactly.

import crypto from 'node:crypto';

export const MODULUS = 1000003;
// Sections of about 820 lines, the mean size of an L5 source file.
const UNITS_PER_SECTION = 6;
const WORDS = Object.freeze(['alpha', 'bravo', 'cedar', 'delta', 'ember', 'fjord', 'grove', 'harbor',
  'island', 'juniper', 'kestrel', 'lantern', 'meadow', 'nectar', 'orchid', 'prairie']);

// mulberry32: a 32-bit PRNG whose stream depends only on the seed.
function random(seed) {
  let state = seed >>> 0;
  const next = () => {
    state = (state + 0x6D2B79F5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return (t ^ (t >>> 14)) >>> 0;
  };
  return { int: (lo, hi) => lo + (next() % (hi - lo + 1)), pick: list => list[next() % list.length] };
}

// Templates are written with short local names; the emitted source spells
// them the way the compiler's code does, which brings lines to L5's length.
const RENAMES = Object.freeze({
  seed: 'seed_value', accumulator: 'running_total', index: 'position', records: 'built_records',
  shapes: 'built_shapes', counts: 'word_counts', table: 'lookup_table', probes: 'probe_keys',
  steps: 'step_count', step: 'step_value', marker: 'marker_value', found: 'found_value',
});
const RENAMED = new RegExp(`\\b(${Object.keys(RENAMES).join('|')})\\b`, 'g');

const mod = value => {
  if (!Number.isSafeInteger(value) || value < 0) throw new Error(`model value left the exact range: ${value}`);
  return value % MODULUS;
};
const blend = (left, right, weight) => mod(left * weight + right);

export function isCodeLine(line) {
  const text = line.trim();
  return text.length > 0 && !text.startsWith('//');
}

export function countLines(text) {
  const lines = text.endsWith('\n') ? text.slice(0, -1).split('\n') : text.split('\n');
  return { physical: lines.length, code: lines.filter(isCodeLine).length };
}

// One unit: a record struct, a shape enum, the section trait's impl, five
// worker functions of different construct mixes, and the unit total.
function unit(m, u, r, stats) {
  const tag = `section_${m}_unit_${u}`;
  const rec = `RecordM${m}U${u}`;
  const shape = `ShapeM${m}U${u}`;
  const trait = `ScoredM${m}`;
  const lines = [];
  const constant = (u % 5 === 0 || u % 5 === 2) ? `SEED_M${m}_U${u}` : null;
  const constantA = r.int(3, 40);
  const constantB = r.int(5, 90);
  const constantValue = constantA * 7 + constantB;
  if (constant) {
    lines.push(`// Prepared once while building; folded into every use below.`);
    lines.push(`${constant} :: prep { ${constantA} * 7 + ${constantB} }`, '');
    stats.comptime_items += 1;
  }
  const scoreA = r.int(3, 19);
  const scoreB = r.int(2, 11);
  lines.push(`struct ${rec} {`, '    identifier: Int', '    weight: Int', '    label: String', '    markers: [Int]', '}', '');
  lines.push(`enum ${shape} {`, '    Dot(Int)', '    Segment(start: Int, finish: Int)', '    Named(String)', '    Empty', '}', '');
  lines.push(`impl ${rec}.${trait} {`,
    `    fn score(self) -> Int { (self.identifier * ${scoreA} + self.weight * ${scoreB} + self.label.len()) % ${MODULUS} }`, '}', '');
  stats.types += 2;
  stats.functions += 1;
  const score = record => mod(record.identifier * scoreA + record.weight * scoreB + record.label.length);

  const workers = [];
  // 1. Records built in a loop, scored through trait values.
  {
    const name = `${tag}_build_records`;
    const count = r.int(3, 6);
    const a = r.int(3, 29);
    const b = r.int(3, 17);
    const c = r.int(1, 13);
    const threshold = r.int(5, 25);
    const index = r.int(0, 7);
    const text = [
      `fn ${name}(seed: Int) -> Int {`,
      `    accumulator := seed % ${MODULUS}`,
      `    records := [${rec}]{}`,
      `    loop index in 0..<${count} {`,
      `        marker :: (index * ${a} + seed) % 97`,
      `        &records.push(${rec}{identifier: index + marker, weight: (index * ${b} + ${c}) % 31, label: "record-{index}-{marker}", markers: [Int]{index, marker}})`,
      '    }',
      `    scored :: [${trait}]{records[0], records[1], records[2]}`,
      `    accumulator = (accumulator + total_score_m${m}(scored) + score_one_m${m}(records[${count - 1}])) % ${MODULUS}`,
      '    loop record in records {',
      '        accumulator = if {',
      `            record.weight > ${threshold} -> (accumulator * 3 + record.markers.len()) % ${MODULUS}`,
      `            else -> (accumulator + record.identifier * 5) % ${MODULUS}`,
      '        }',
      '    }',
      `    fallback :: ${rec}{identifier: 0, weight: 0, label: "none", markers: [Int]{}}`,
      `    chosen :: pick_m${m}(records, ${index}, fallback)`,
      `    accumulator = (accumulator + chosen.identifier + chosen.label.len()) % ${MODULUS}`,
      '    accumulator',
      '}',
    ];
    const model = seed => {
      let accumulator = mod(seed);
      const records = [];
      for (let index = 0; index < count; index++) {
        const marker = (index * a + seed) % 97;
        records.push({ identifier: index + marker, weight: (index * b + c) % 31, label: `record-${index}-${marker}`, markers: [index, marker] });
      }
      accumulator = mod(accumulator + mod(mod(score(records[0]) + score(records[1])) + score(records[2])) + mod(score(records[count - 1]) * 2));
      for (const record of records) {
        accumulator = record.weight > threshold ? mod(accumulator * 3 + record.markers.length) : mod(accumulator + record.identifier * 5);
      }
      const chosen = index < records.length ? records[index] : { identifier: 0, label: 'none' };
      return mod(accumulator + chosen.identifier + chosen.label.length);
    };
    workers.push({ name, text, model });
  }
  // 2. Enum values built by a value match, consumed by a variant match.
  {
    const name = `${tag}_match_shapes`;
    const count = r.int(4, 8);
    const a = r.int(3, 41);
    const b = r.int(2, 23);
    const c = r.int(1, 19);
    const d = r.int(2, 9);
    const e = r.int(7, 97);
    const text = [
      `fn ${name}(seed: Int) -> Int {`,
      `    accumulator := (seed * ${a}) % ${MODULUS}`,
      `    shapes := [${shape}]{}`,
      `    loop index in 0..<${count} {`,
      `        next_shape :: if (seed + index) % 4 == {`,
      `            0 -> ${shape}.Dot((seed + index * ${b}) % 101)`,
      `            1 -> ${shape}.Segment{start: index, finish: (seed + ${c}) % 53}`,
      `            2 -> ${shape}.Named("shape-{index}-{seed % 7}")`,
      `            else -> ${shape}.Empty`,
      '        }',
      '        &shapes.push(next_shape)',
      '    }',
      '    loop item in shapes {',
      '        step :: if item == {',
      `            .Dot(value) -> value * ${d}`,
      '            .Segment(start, finish) -> start * 5 + finish',
      '            .Named(text) -> text.len() * 7',
      `            .Empty -> ${e}`,
      '        }',
      `        accumulator = (accumulator * 31 + step) % ${MODULUS}`,
      '    }',
      '    accumulator',
      '}',
    ];
    const model = seed => {
      let accumulator = mod(seed * a);
      for (let index = 0; index < count; index++) {
        const k = (seed + index) % 4;
        const step = k === 0 ? ((seed + index * b) % 101) * d
          : k === 1 ? index * 5 + (seed + c) % 53
            : k === 2 ? `shape-${index}-${seed % 7}`.length * 7 : e;
        accumulator = mod(accumulator * 31 + step);
      }
      return accumulator;
    };
    workers.push({ name, text, model });
  }
  // 3. String keys, interpolation, and a map read through `??`.
  {
    const name = `${tag}_count_words`;
    const words = [r.pick(WORDS), r.pick(WORDS), r.pick(WORDS), r.pick(WORDS)];
    const buckets = r.int(2, 5);
    const rounds = r.int(3, 6);
    const a = r.int(3, 9);
    const limit = r.int(20, 32);
    const text = [
      `fn ${name}(seed: Int) -> Int {`,
      '    counts := [String:Int]{}',
      `    words :: [String]{${words.map(word => `"${word}"`).join(', ')}}`,
      '    loop (index, word) in words {',
      `        key :: "{word}-{(seed + index) % ${buckets}}"`,
      '        counts[key] = (counts.get(key) ?? 0) + index + 1',
      '    }',
      '    accumulator := counts.len()',
      '    loop (key, value) in counts {',
      `        accumulator = (accumulator + key.len() * value * 17) % ${MODULUS}`,
      '    }',
      `    label := "${tag}"`,
      `    loop index in 0..<${rounds} {`,
      `        label = "{label}:{index * ${a} % 10}"`,
      '    }',
      '    accumulator = if {',
      `        label.len() > ${limit} -> (accumulator + label.len()) % ${MODULUS}`,
      `        else -> (accumulator + 3) % ${MODULUS}`,
      '    }',
      '    accumulator',
      '}',
    ];
    const model = seed => {
      const counts = new Map();
      words.forEach((word, index) => {
        const key = `${word}-${(seed + index) % buckets}`;
        counts.set(key, (counts.get(key) ?? 0) + index + 1);
      });
      let accumulator = counts.size;
      for (const [key, value] of counts) accumulator = mod(accumulator + key.length * value * 17);
      let label = tag;
      for (let index = 0; index < rounds; index++) label = `${label}:${index * a % 10}`;
      return label.length > limit ? mod(accumulator + label.length) : mod(accumulator + 3);
    };
    workers.push({ name, text, model });
  }
  // 4. Optional values mapped through closures, and a local closure.
  {
    const name = `${tag}_map_options`;
    const keys = [r.int(1, 30), r.int(31, 60), r.int(61, 90)];
    const values = [r.int(1, 999), r.int(1, 999), r.int(1, 999)];
    const rounds = r.int(3, 7);
    const spread = r.int(3, 11);
    const fallback = r.int(1, 50);
    const factor = r.int(2, 13);
    const offset = r.int(1, 99);
    const text = [
      `fn ${name}(seed: Int) -> Int {`,
      `    table :: [Int:Int]{${keys.map((key, i) => `${key}: ${values[i]}`).join(', ')}}`,
      `    probes :: [Int]{${keys[0]}, ${keys[1]}, ${keys[2]}, seed % 100}`,
      `    accumulator := seed % ${MODULUS}`,
      `    loop round in 0..<${rounds} {`,
      `        probe :: probes[round % 4] + (round /% 4) * ${spread}`,
      '        found :: table.get(probe)',
      `        accumulator = (accumulator + round + (found.map((value) -> value * 2) ?? ${fallback})) % ${MODULUS}`,
      '    }',
      `    adjust :: (value: Int) -> (value * ${factor} + ${constantValue}) % ${MODULUS}`,
      '    accumulator = adjust(accumulator)',
      '    first :: probes.first()',
      `    accumulator = (accumulator + (first.map((value) -> value + ${offset}) ?? 0)) % ${MODULUS}`,
      '    accumulator',
      '}',
    ];
    const model = seed => {
      const table = new Map(keys.map((key, i) => [key, values[i]]));
      const probes = [keys[0], keys[1], keys[2], seed % 100];
      let accumulator = mod(seed);
      for (let round = 0; round < rounds; round++) {
        const probe = probes[round % 4] + Math.floor(round / 4) * spread;
        const found = table.get(probe);
        accumulator = mod(accumulator + round + (found === undefined ? fallback : found * 2));
      }
      accumulator = mod(accumulator * factor + constantValue);
      return mod(accumulator + probes[0] + offset);
    };
    workers.push({ name, text, model });
  }
  // 5. Integer loops and ordered arm tables.
  {
    const name = `${tag}_settle_numbers`;
    const limit = r.int(20, 60);
    const a = r.int(2, 9);
    const b = r.int(1, 30);
    const text = [
      `fn ${name}(seed: Int) -> Int {`,
      '    value := seed % 1000 + 2',
      '    steps := 0',
      `    loop value > 1 && steps < ${limit} {`,
      '        value = if {',
      '            value % 2 == 0 -> value /% 2',
      '            else -> value * 3 + 1',
      '        }',
      '        steps += 1',
      '    }',
      `    weight := ${b}`,
      '    loop step in 0..<steps {',
      '        weight = if step % 3 == {',
      `            0 -> (weight * ${a} + step) % ${MODULUS}`,
      '            1 -> (weight + step * step) % 1000003',
      '            else -> (weight + 7) % 1000003',
      '        }',
      '    }',
      `    (weight * 1000 + steps + value) % ${MODULUS}`,
      '}',
    ];
    const model = seed => {
      let value = seed % 1000 + 2;
      let steps = 0;
      while (value > 1 && steps < limit) {
        value = value % 2 === 0 ? value / 2 : value * 3 + 1;
        steps += 1;
      }
      let weight = b;
      for (let step = 0; step < steps; step++) {
        const k = step % 3;
        weight = k === 0 ? mod(weight * a + step) : k === 1 ? mod(weight + step * step) : mod(weight + 7);
      }
      return mod(weight * 1000 + steps + value);
    };
    workers.push({ name, text, model });
  }

  const start = constant ?? String(constantValue);
  const totalText = [`fn ${tag}_total() -> Int {`, `    accumulator := ${start} % ${MODULUS}`];
  let accumulator = mod(constantValue);
  for (const [index, worker] of workers.entries()) {
    // Every worker folds its result through the section's three-argument
    // blend (the call shape of the compiler's own helpers) and a short trace
    // label, as the compiler's passes report what they did.
    const first = r.int(2, 97);
    const second = r.int(2, 97);
    const third = r.int(2, 97);
    const fourth = r.int(2, 97);
    const labelLimit = r.int(34, 42);
    const tail = worker.text[worker.text.length - 2].trim();
    const text = [...worker.text.slice(0, -2),
      `    worker_result :: ${tail}`,
      `    checkpoint :: blend_m${m}(worker_result, seed % 1000, ${first})`,
      `    trace_label :: "${worker.name}:{worker_result % 1000}:{checkpoint % 1000}"`,
      `    trace_weight :: if trace_label.len() > ${labelLimit} -> blend_m${m}(checkpoint, trace_label.len(), ${third}) else -> blend_m${m}(checkpoint, 1, ${third})`,
      '    blended_parts :: [Int]{worker_result % 997, checkpoint % 991, trace_weight % 983}',
      '    combined := 0',
      `    loop part in blended_parts -> combined = blend_m${m}(combined, part, ${fourth})`,
      `    blend_m${m}(combined, worker_result % 997, ${second})`,
      '}'];
    const model = seed => {
      const result = worker.model(seed);
      const checkpoint = blend(result, seed % 1000, first);
      const labelLength = `${worker.name}:${result % 1000}:${checkpoint % 1000}`.length;
      const traceWeight = blend(checkpoint, labelLength > labelLimit ? labelLength : 1, third);
      let combined = 0;
      for (const part of [result % 997, checkpoint % 991, traceWeight % 983]) combined = blend(combined, part, fourth);
      return blend(combined, result % 997, second);
    };
    if (index % 2 === 0) lines.push(`// Worker ${index} of ${tag}.`);
    lines.push(...text.map(line => line.replace(RENAMED, name => RENAMES[name])), '');
    totalText.push(`    accumulator = (accumulator * 7 + ${worker.name}(accumulator + ${index + 1})) % ${MODULUS}`);
    accumulator = mod(accumulator * 7 + model(accumulator + index + 1));
    stats.functions += 1;
  }
  totalText.push('    accumulator', '}');
  lines.push(...totalText, '');
  stats.functions += 1;
  return { lines, value: accumulator, totalName: `${tag}_total` };
}

function sectionHeader(m) {
  const trait = `ScoredM${m}`;
  return [
    `// Section ${m} of the mixed compile-throughput benchmark.`,
    `trait ${trait} {`, '    fn score(self) -> Int', '}', '',
    `fn pick_m${m}<T>(items: [T], index: Int, fallback: T) -> T {`,
    '    if index < items.len() -> return items[index]',
    '    fallback',
    '}', '',
    `fn total_score_m${m}(items: [${trait}]) -> Int {`,
    '    sum := 0',
    `    loop item in items -> sum = (sum + item.score()) % ${MODULUS}`,
    '    sum',
    '}', '',
    `fn score_one_m${m}(item: ${trait}) -> Int { item.score() * 2 }`, '',
    `fn blend_m${m}(left: Int, right: Int, weight: Int) -> Int { (left * weight + right) % ${MODULUS} }`, '',
  ];
}

const PACKAGE_MANIFEST = 'name: "mixed_bench"\nversion: "0.1.0"\nedition: "2028"\noutputs: { app: .Executable{ entry: run } }\n';

// The package for `codeLines` code lines: package.jet plus main.jet, filled
// section by section, unit by unit, until main.jet reaches the target.
export function mixedSources(codeLines, seed) {
  if (!Number.isSafeInteger(codeLines) || codeLines < 1) throw new Error('--code-lines must be a positive safe integer');
  if (!Number.isSafeInteger(seed) || seed < 0 || seed > 0xFFFFFFFF) throw new Error('--seed must be an integer in 0..4294967295');
  const r = random(seed);
  const stats = { functions: 1, types: 0, comptime_items: 0, sections: 0 };
  const lines = [`// Deterministic mixed compile-throughput workload: --code-lines ${codeLines} --seed ${seed}`, ''];
  const sectionTotals = [];
  const codeOf = list => list.filter(isCodeLine).length;
  // `fn run() {`, `checksum := 0`, the print and `}`, plus one sum line per section.
  let code = 4;
  while (code < codeLines) {
    const m = stats.sections;
    const header = sectionHeader(m);
    lines.push(...header);
    code += codeOf(header) + 1;
    stats.functions += 4;
    stats.types += 1;
    const units = [];
    for (let u = 0; u < UNITS_PER_SECTION && (u === 0 || code < codeLines); u++) {
      const generated = unit(m, u, r, stats);
      lines.push(...generated.lines);
      code += codeOf(generated.lines);
      units.push(generated);
    }
    const total = [`fn section_total_m${m}() -> Int {`, '    accumulator := 0'];
    let value = 0;
    for (const generated of units) {
      total.push(`    accumulator = (accumulator * 13 + ${generated.totalName}()) % ${MODULUS}`);
      value = mod(value * 13 + generated.value);
    }
    total.push('    accumulator', '}', '');
    lines.push(...total);
    code += codeOf(total);
    stats.functions += 1;
    sectionTotals.push(value);
    stats.sections += 1;
  }
  let checksum = 0;
  lines.push('fn run() {', '    checksum := 0');
  for (const [m, value] of sectionTotals.entries()) {
    lines.push(`    checksum = (checksum * 31 + section_total_m${m}()) % ${MODULUS}`);
    checksum = mod(checksum * 31 + value);
  }
  lines.push(`    print("mixed checksum {checksum} sections ${stats.sections}")`, '}');
  const files = { 'package.jet': PACKAGE_MANIFEST, 'main.jet': `${lines.join('\n')}\n` };
  const expected = `mixed checksum ${checksum} sections ${stats.sections}\n`;
  return { files, expected, stats: { ...mixedStatistics(files['main.jet']), ...stats }, source_sha256: sourceDigest(files) };
}

// One digest over the whole package: each file's name and bytes, by name.
export function sourceDigest(files) {
  const hash = crypto.createHash('sha256');
  for (const name of Object.keys(files).sort()) hash.update(`${name}\0${files[name]}\0`);
  return hash.digest('hex');
}

// Line and byte shape of the Jet source (the manifest is not program text).
function mixedStatistics(text) {
  const lines = countLines(text);
  return { physical_lines: lines.physical, code_lines: lines.code, bytes: Buffer.byteLength(text) };
}
