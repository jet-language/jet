// Corpus family of the compiler scaling gate (Tower #4601).
//
// Every shape writes one plain, idiomatic Jet package that neither compiler
// was tuned for. A *stress* shape grows exactly one dimension with N (files,
// functions, statements per function, nesting depth, enum width, impls,
// instantiations, comptime items, table rows, imports, call depth, fallible
// functions, diagnostics, string holes, call sites); everything else stays
// fixed or grows only as the consumer code that keeps the new items live.
// A *domain* shape (domains.mjs) is a realistic program of one kind (web
// service, game, CLI, data analysis, GUI, embedded, compiler, scientific,
// generic library, generated code) sized by code lines.
//
// Adding a shape: write `build(n, r)` returning `{ files, expect }` (or a
// domain in domains.mjs) and register it in SHAPES with its dimension, unit
// and default base. `files` maps package-relative paths to text; the
// package's `run: fn()` must reach every generated function, so no phase can
// skip work as dead code. `expect.diagnostics` (diagnostics shape only) is
// the number of errors a correct compiler reports.
//
// Both compilers build these packages: one package, one flat namespace across
// its files (E0624 forbids the same top-level name in two files), no path
// dependencies (the bootstrap runner authorizes one source root), no bounded
// trait method calls through a type parameter and no mutation inside `#run`
// (both unsupported by the generated compiler today).

import { DOMAINS } from './domains.mjs';
import { MODULUS, codeLines, consumerTree, finish, pack, random, sourceDigest } from './package.mjs';

const M = MODULUS;

// ---------------------------------------------------------------- stress

function modules(n, r) {
  const files = {};
  const entries = [];
  for (let i = 0; i < n; i++) {
    const a = r.int(3, 97);
    const b = r.int(2, 31);
    files[`modules/part_${i}.jet`] = [
      `Record${i}: struct {`, '    id: Int', '    weight: Int', '}', '',
      `record_${i}_make: fn(seed: Int) -> Record${i} { Record${i}{id: seed % 97, weight: (seed * ${a}) % 89} }`, '',
      `record_${i}_score: fn(seed: Int) -> Int {`,
      `    record: record_${i}_make(seed)`,
      `    (record.id * ${b} + record.weight) % ${M}`, '}', '',
    ].join('\n');
    entries.push(`record_${i}_score`);
  }
  return finish('modules', files, consumerTree(files, entries, 'modules'));
}

const SMALL_BODIES = [
  (i, a, b) => [`helper_${i}: fn(x: Int) -> Int { (x * ${a} + ${b}) % ${M} }`],
  (i, a, b) => [`helper_${i}: fn(x: Int) -> Int {`, `    if x % ${a} == 0 -> return x /% ${a}`, `    x + ${b}`, '}'],
  (i, a, b) => [`helper_${i}: fn(x: Int) -> Int {`, `    label: "h${i}-{x % ${a}}"`, `    label.len() + ${b}`, '}'],
  (i, a, b) => [`helper_${i}: fn(x: Int) -> Int {`, '    total := 0', `    loop k in 0..<${a % 4 + 1} -> total += x + k * ${b}`, `    total % ${M}`, '}'],
];

function smallFns(n, r) {
  const files = {};
  const perFile = Math.ceil(n / 16);
  const items = [];
  const entries = [];
  for (let i = 0; i < n; i++) {
    items.push(SMALL_BODIES[i % SMALL_BODIES.length](i, r.int(2, 97), r.int(1, 63)));
    entries.push(`helper_${i}`);
  }
  for (let k = 0; k < 16; k++) files[`fns/fns_${k}.jet`] = items.slice(k * perFile, (k + 1) * perFile).map(lines => lines.join('\n')).join('\n\n');
  return finish('small_fns', files, consumerTree(files, entries, 'small'));
}

// Four functions of n/4 statements each: straight-line updates, branches and
// a growing set of locals that later statements read.
function hugeFns(n, r) {
  const files = {};
  const entries = [];
  for (let f = 0; f < 4; f++) {
    const lines = [`huge_${f}: fn(seed: Int) -> Int {`, `    acc := seed % ${M}`];
    const locals = [];
    for (let s = 0; s < n / 4; s++) {
      const c = r.int(1, 997);
      switch (s % 5) {
        case 0: lines.push(`    acc = (acc * 31 + ${c}) % ${M}`); break;
        case 1: lines.push(`    if acc % ${r.int(2, 13)} == 0 -> acc = (acc + ${c}) % ${M}`); break;
        case 2: lines.push(`    t${s}: (acc + ${c}) % 1009`); locals.push(`t${s}`); break;
        case 3: lines.push(`    acc = (acc + ${locals.length ? r.pick(locals) : c} * ${r.int(2, 9)}) % ${M}`); break;
        default: lines.push(`    acc = if acc > ${c * 1000} -> acc - ${c} else -> acc + ${c}`);
      }
    }
    lines.push('    acc', '}', '');
    files[`huge/huge_${f}.jet`] = lines.join('\n');
    entries.push(`huge_${f}`);
  }
  return finish('huge_fns', files, entries);
}

// Nesting depth n: statement blocks (if / loop / value-if) and one
// parenthesized expression, each n deep, in four functions.
function deepNesting(n, r) {
  const files = {};
  const lines = [];
  for (let f = 0; f < 3; f++) {
    lines.push(`nested_${f}: fn(seed: Int) -> Int {`, `    acc := seed % ${M}`);
    let indent = '    ';
    for (let d = 0; d < n; d++) {
      const kind = (d + f) % 3;
      if (kind === 0) lines.push(`${indent}if acc % ${r.int(2, 7)} != ${r.int(7, 9)} {`);
      else if (kind === 1) lines.push(`${indent}loop k${d} in 0..<1 {`);
      else lines.push(`${indent}if acc >= 0 {`);
      indent += '    ';
      lines.push(`${indent}acc = (acc * 3 + ${kind === 1 ? `k${d} + ` : ''}${r.int(1, 99)}) % ${M}`);
    }
    for (let d = n - 1; d >= 0; d--) {
      indent = indent.slice(4);
      lines.push(`${indent}}`);
    }
    lines.push('    acc', '}', '');
  }
  lines.push(`nested_expr: fn(seed: Int) -> Int { ${'(1 + '.repeat(n)}seed${')'.repeat(n)} % ${M} }`, '');
  files['nested.jet'] = lines.join('\n');
  return finish('deep_nesting', files, ['nested_0', 'nested_1', 'nested_2', 'nested_expr']);
}

// One enum of n variants and four n-arm tables over it.
function wideEnums(n, r) {
  const variant = i => ['Plain', 'Count', 'Span', 'Named'][i % 4] + i;
  const files = {};
  const decl = ['Opcode: enum {'];
  for (let i = 0; i < n; i++) {
    const v = variant(i);
    decl.push(i % 4 === 0 ? `    ${v}` : i % 4 === 1 ? `    ${v}(Int)` : i % 4 === 2 ? `    ${v}(start: Int, width: Int)` : `    ${v}(String)`);
  }
  decl.push('}', '');
  const cost = ['opcode_cost: fn(op: Opcode) -> Int {', '    if op == {'];
  const name = ['opcode_name: fn(op: Opcode) -> String {', '    if op == {'];
  const from = ['opcode_from_code: fn(code: Int) -> Opcode {', '    if code == {'];
  const keyword = ['keyword_code: fn(text: String) -> Int {', '    if text == {'];
  for (let i = 0; i < n; i++) {
    const v = variant(i);
    const c = r.int(1, 97);
    cost.push(i % 4 === 0 ? `        .${v} -> ${c}` : i % 4 === 1 ? `        .${v}(k) -> k * ${c}` : i % 4 === 2 ? `        .${v}(start, width) -> start + width * ${c}` : `        .${v}(text) -> text.len() + ${c}`);
    name.push(`        .${v}${i % 4 === 0 ? '' : i % 4 === 2 ? '(_, _)' : '(_)'} -> "${v.toLowerCase()}"`);
    from.push(`        ${i} -> ${i % 4 === 0 ? `Opcode.${v}` : i % 4 === 1 ? `Opcode.${v}(${c})` : i % 4 === 2 ? `Opcode.${v}{start: ${c}, width: ${i % 7 + 1}}` : `Opcode.${v}("op${i}")`}`);
    keyword.push(`        "${v.toLowerCase()}" -> ${i}`);
  }
  cost.push('    }', '}', '');
  name.push('    }', '}', '');
  from.push(`        else -> Opcode.${variant(0)}`, '    }', '}', '');
  keyword.push('        else -> 0', '    }', '}', '');
  const consumer = ['opcode_total: fn(seed: Int) -> Int {', `    acc := seed % ${M}`, `    loop code in 0..<${n} {`,
    '        op: opcode_from_code(code)',
    `        acc = (acc * 7 + opcode_cost(op) + keyword_code(opcode_name(op))) % ${M}`, '    }', '    acc', '}', ''];
  files['opcodes.jet'] = [...decl, ...cost, ...name, ...from, ...keyword, ...consumer].join('\n');
  return finish('wide_enums', files, ['opcode_total']);
}

// n types, n/8 traits (at least 4), four trait impls and one inherent impl
// per type, trait values consumed per trait.
function implsTraits(n, r) {
  const files = {};
  const traitCount = Math.max(4, Math.floor(n / 8));
  const traits = [];
  for (let k = 0; k < traitCount; k++) {
    traits.push([`Capability${k}: trait {`, `    measure_${k}: fn(self) -> Int`, `    label_${k}: fn(self) -> String`, '}', '',
      `capability_${k}_total: fn(items: [Capability${k}]) -> Int {`, '    total := 0',
      `    loop item in items -> total = (total + item.measure_${k}() + item.label_${k}().len()) % ${M}`, '    total', '}']);
  }
  pack(files, 'traits/traits', traits);
  const types = [];
  const entries = [];
  for (let i = 0; i < n; i++) {
    const lines = [`Entity${i}: struct {`, '    id: Int', '    name: String', '}', ''];
    const chosen = [];
    for (let j = 0; j < 4; j++) {
      const k = (i * 4 + j) % traitCount;
      if (chosen.includes(k)) continue;
      chosen.push(k);
      lines.push(`impl Entity${i}.Capability${k} {`,
        `    measure_${k}: fn(self) -> Int { (self.id * ${r.int(2, 97)} + ${r.int(1, 99)}) % ${M} }`,
        `    label_${k}: fn(self) -> String { "e${i}-c${k}-{self.name}" }`, '}', '');
    }
    lines.push(`impl Entity${i} {`, `    bump: fn(&self) { self.id += ${r.int(1, 9)} }`, '    weight: fn(self) -> Int { self.id + self.name.len() }', '}', '',
      `entity_${i}_use: fn(seed: Int) -> Int {`, `    entity := Entity${i}{id: seed % 101, name: "n{seed % 13}"}`, '    &entity.bump()', '    total := entity.weight()');
    for (const k of chosen) lines.push(`    total = (total + capability_${k}_total([Capability${k}]{entity})) % ${M}`);
    lines.push('    total', '}');
    types.push(lines);
    entries.push(`entity_${i}_use`);
  }
  pack(files, 'types/entities', types);
  return finish('impls_traits', files, consumerTree(files, entries, 'impls'));
}

// A fixed generic library instantiated with n distinct record types, all in
// the entry file: generic code outside the entry file does not compile today
// (domains.mjs explains the two compilers' errors).
function generics(n, r) {
  const files = {};
  const entries = [];
  const lines = [
    'Slot: struct<T> {', '    value: T', '    mark: Int', '}', '',
    'wrap: fn<T>(value: T, mark: Int) -> Slot<T> { Slot<T>{value: value, mark: mark} }', '',
    'first_or: fn<T>(items: [T], fallback: T) -> T {', '    if items.len() > 0 -> return items[0]', '    fallback', '}', '',
    'count_where: fn<T>(items: [T], keep: fn(T) -> Bool) -> Int {', '    total := 0', '    loop item in items {', '        if keep(item) -> total += 1', '    }', '    total', '}', '',
    'marks_of: fn<T>(slots: [Slot<T>]) -> Int {', '    total := 0', '    loop slot in slots -> total += slot.mark', '    total', '}', '',
  ];
  for (let i = 0; i < n; i++) {
    const a = r.int(2, 97);
    lines.push(`Thing${i}: struct {`, '    id: Int', '    weight: Int', '}', '',
      `thing_${i}_use: fn(seed: Int) -> Int {`,
      `    things: [Thing${i}]{Thing${i}{id: seed % 31, weight: ${a}}, Thing${i}{id: seed % 17, weight: ${a + 1}}}`,
      `    chosen: first_or(things, Thing${i}{id: 0, weight: 0})`,
      `    heavy: count_where(things, fn(thing) { thing.weight > ${a} })`,
      `    slots: [Slot<Thing${i}>]{wrap(chosen, ${i % 64}), wrap(things[1], heavy)}`,
      `    (chosen.id + heavy + marks_of(slots)) % ${M}`, '}', '');
    entries.push(`thing_${i}_use`);
  }
  files['main.jet'] = lines.join('\n');
  return finish('generics', files, consumerTree(files, entries, 'generics'));
}

// n comptime constants (a third chained on the previous one) with n/2
// record types whose methods read them.
function comptime(n, r) {
  const files = {};
  const items = [];
  const entries = [];
  for (let i = 0; i < n; i++) {
    const a = r.int(3, 40);
    const b = r.int(5, 90);
    const lines = [i % 3 === 2 ? `LIMIT_${i}: #run { LIMIT_${i - 1} * 2 + ${b} }` : `LIMIT_${i}: #run { ${a} * 7 + ${b} }`, ''];
    if (i % 2 === 0) {
      lines.push(`Gauge${i}: struct {`, '    level: Int', '}', '', `impl Gauge${i} {`,
        `    clamp: fn(self) -> Int { if self.level > LIMIT_${i} -> LIMIT_${i} else -> self.level }`, '}', '',
        `gauge_${i}_use: fn(seed: Int) -> Int { Gauge${i}{level: seed % 300}.clamp() + LIMIT_${i} }`);
      entries.push(`gauge_${i}_use`);
    } else {
      lines.push(`limit_${i}_use: fn(seed: Int) -> Int { (seed + LIMIT_${i}) % ${M} }`);
      entries.push(`limit_${i}_use`);
    }
    items.push(lines);
  }
  pack(files, 'consts/consts', items);
  return finish('comptime', files, consumerTree(files, entries, 'comptime'));
}

// n table elements: four Int tables, a record table and a string-keyed map,
// as functions returning literal tables, plus one long string literal.
function dataTables(n, r) {
  const files = {};
  const lines = [];
  const quarter = Math.max(16, Math.floor(n / 4));
  for (let k = 0; k < 4; k++) {
    lines.push(`table_${k}: fn() -> [Int] {`, '    [Int]{');
    for (let start = 0; start < quarter; start += 16) {
      lines.push(`        ${Array.from({ length: Math.min(16, quarter - start) }, () => r.int(0, 99999)).join(', ')},`);
    }
    lines.push('    }', '}', '');
  }
  const rows = Math.max(8, Math.floor(n / 8));
  lines.push('Row: struct {', '    id: Int', '    weight: Int', '    name: String', '}', '', 'row_table: fn() -> [Row] {', '    [Row]{');
  for (let i = 0; i < rows; i++) lines.push(`        Row{id: ${i}, weight: ${r.int(0, 999)}, name: "row-${i}"},`);
  lines.push('    }', '}', '', 'name_table: fn() -> [String:Int] {', '    [String:Int]{');
  for (let i = 0; i < rows; i++) lines.push(`        "key-${i}": ${r.int(0, 999)},`);
  lines.push('    }', '}', '', `banner: fn() -> String { "${'abcdefgh'.repeat(n)}" }`, '',
    'tables_total: fn(seed: Int) -> Int {', `    acc := seed % ${M}`,
    `    loop k in 0..<4 {`, '        table: if k == {', '            0 -> table_0()', '            1 -> table_1()', '            2 -> table_2()', '            else -> table_3()', '        }',
    `        loop value in table -> acc = (acc * 3 + value) % ${M}`, '    }',
    `    loop row in row_table() -> acc = (acc + row.weight + row.name.len()) % ${M}`,
    `    loop (key, value) in name_table() -> acc = (acc + key.len() * value) % ${M}`,
    `    (acc + banner().len()) % ${M}`, '}', '');
  files['tables.jet'] = lines.join('\n');
  return finish('data_tables', files, ['tables_total']);
}

// core.time is left out: the generated compiler rejects Core/Time today
// (E0312/E0109 on LocalDate comparisons).
const CORE_IMPORTS = ['use math: core.math', 'use text: core.text', 'use collections: core.collections', 'use term: core.term'];

// n files, each importing four Core modules.
function imports(n, r) {
  const files = {};
  const entries = [];
  for (let i = 0; i < n; i++) {
    files[`services/service_${i}.jet`] = [...CORE_IMPORTS, '',
      `service_${i}_value: fn(seed: Int) -> Int { max(seed % 13, ${r.int(1, 12)}) + math.abs_diff(seed, ${r.int(1, 99)}) }`, ''].join('\n');
    entries.push(`service_${i}_value`);
  }
  return finish('imports', files, consumerTree(files, entries, 'imports'));
}

// One call chain n deep across eight files.
function callChains(n, r) {
  const files = {};
  const perFile = Math.ceil(n / 8);
  for (let f = 0; f < 8; f++) {
    const lines = [];
    for (let i = f * perFile; i < Math.min(n, (f + 1) * perFile); i++) {
      lines.push(i === n - 1 ? `chain_${i}: fn(x: Int) -> Int { x % ${M} }`
        : `chain_${i}: fn(x: Int) -> Int { (chain_${i + 1}((x * ${r.int(2, 9)} + ${r.int(1, 99)}) % ${M}) + 1) % ${M} }`, '');
    }
    files[`chain/chain_${f}.jet`] = lines.join('\n');
  }
  return finish('call_chains', files, ['chain_0']);
}

// n fallible functions: typed error families (one per 16 functions),
// propagation, recovery by `??` and by result tables, inferred text errors.
function errors(n, r) {
  const files = {};
  const items = [];
  const entries = [];
  for (let i = 0; i < n; i++) {
    const k = Math.floor(i / 16);
    const lines = [];
    if (i % 16 === 0) {
      lines.push('#Error', `Fault${k}: enum {`, '    Missing', '    Invalid(Int)', '    Overflow(limit: Int, seen: Int)', '}', '',
        `fault_${k}_code: fn(fault: Fault${k}) -> Int {`, '    if fault == {', '        .Missing -> 1', '        .Invalid(value) -> value % 97', '        .Overflow(limit, seen) -> limit + seen', '    }', '}', '');
    }
    const limit = r.int(50, 500);
    lines.push(`parse_${i}: fn(x: Int) -> Int Fault${k}! {`,
      `    if x == 0 -> return Err(Fault${k}.Missing)`,
      `    if x % ${r.int(5, 17)} == 0 -> return Err(Fault${k}.Invalid(x))`,
      `    if x > ${limit * 100} -> return Err(Fault${k}.Overflow{limit: ${limit}, seen: x})`,
      '    Ok(x * 2)', '}', '',
      `step_${i}: fn(x: Int) -> Int Fault${k}! {`, `    first: parse_${i}(x)`, `    second: parse_${i}(first + 1)`, '    Ok(first + second)', '}', '',
      `checked_${i}: fn(x: Int) -> Int {`, `    if x < 0 -> return Err("negative input ${i}")`, `    x % ${M}`, '}', '',
      `recover_${i}: fn(seed: Int) -> Int {`, `    quick: step_${i}(seed % 1000) ?? 0`,
      `    detailed: if step_${i}(seed % 77) == {`, '        .Ok(value) -> value', `        .Err(fault) -> fault_${k}_code(fault)`, '    }',
      `    (quick + detailed + (checked_${i}(seed) ?? 0)) % ${M}`, '}');
    items.push(lines);
    entries.push(`recover_${i}`);
  }
  pack(files, 'errors/errors', items);
  return finish('errors', files, consumerTree(files, entries, 'errors'));
}

// n functions with one undefined name each: n diagnostics expected.
function diagnostics(n, r) {
  const files = {};
  const items = [];
  const entries = [];
  for (let i = 0; i < n; i++) {
    items.push([`broken_${i}: fn(seed: Int) -> Int {`, `    base: seed * ${r.int(2, 9)}`, `    base + missing_value_${i}`, '}']);
    entries.push(`broken_${i}`);
  }
  pack(files, 'broken/broken', items);
  const built = finish('diagnostics', files, consumerTree(files, entries, 'diagnostics'));
  return { files: built, expect: { diagnostics: n } };
}

// n interpolation holes and 64n literal bytes, in n/16 functions.
function strings(n, r) {
  const files = {};
  const items = [];
  const entries = [];
  const fns = Math.max(1, Math.floor(n / 16));
  for (let i = 0; i < fns; i++) {
    items.push([`text_${i}: fn(x: Int) -> Int {`,
      `    line: "${Array.from({ length: 16 }, (_, h) => `part${h}={x + ${h}} `).join('')}"`,
      `    literal: "${'lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiu'.slice(0, 64).repeat(16)}"`,
      `    (line.len() + literal.len() + ${r.int(1, 9)}) % ${M}`, '}']);
    entries.push(`text_${i}`);
  }
  pack(files, 'texts/texts', items);
  return finish('strings', files, consumerTree(files, entries, 'strings'));
}

// n call sites into two shared helpers.
function fanIn(n, r) {
  const files = {};
  files['shared.jet'] = ['shared_scale: fn(x: Int) -> Int { (x * 3 + 1) % 1000003 }', '', 'shared_label: fn(x: Int) -> Int { "v{x % 100}".len() }', ''].join('\n');
  const items = [];
  const entries = [];
  for (let g = 0; g < Math.ceil(n / 32); g++) {
    const lines = [`callers_${g}: fn(seed: Int) -> Int {`, `    acc := seed % ${M}`];
    for (let c = 0; c < 32 && g * 32 + c < n; c++) {
      lines.push(c % 2 === 0 ? `    acc = (acc + shared_scale(acc + ${r.int(1, 99)})) % ${M}` : `    acc = (acc + shared_label(acc)) % ${M}`);
    }
    lines.push('    acc', '}');
    items.push(lines);
    entries.push(`callers_${g}`);
  }
  pack(files, 'callers/callers', items);
  return finish('fan_in', files, consumerTree(files, entries, 'fan_in'));
}

// ---------------------------------------------------------------- registry

const stress = (dimension, base, build) => ({ kind: 'stress', dimension, unit: 'n', base, build });
export const SHAPES = Object.freeze({
  modules: stress('files in one package', 128, modules),
  'small-fns': stress('small functions', 1024, smallFns),
  'huge-fns': stress('statements in four functions', 2048, hugeFns),
  'deep-nesting': stress('block and expression nesting depth', 16, deepNesting),
  'wide-enums': stress('enum variants and arms per match table', 64, wideEnums),
  'impls-traits': stress('types with four trait impls each (traits = n/8)', 128, implsTraits),
  generics: stress('distinct generic instantiations', 128, generics),
  comptime: stress('comptime (prep) items with record types', 128, comptime),
  'data-tables': stress('literal table elements', 2048, dataTables),
  imports: stress('files importing five Core modules', 128, imports),
  'call-chains': stress('call depth', 256, callChains),
  errors: stress('fallible functions with propagation and recovery', 128, errors),
  diagnostics: stress('reported errors', 128, diagnostics),
  strings: stress('interpolation holes (64 literal bytes each)', 1024, strings),
  'fan-in': stress('call sites into shared helpers', 1024, fanIn),
  ...Object.fromEntries(Object.entries(DOMAINS).map(([name, domain]) => [name, {
    kind: 'domain', dimension: domain.dimension, unit: 'lines', base: 125000, build: domain.build,
  }])),
});

// The package of `shape` at size `n` (N for stress shapes, code lines for
// domain shapes): files, expected diagnostics, code lines and digest.
// `layout` is `files` (as generated: many files, the realistic layout) or
// `single` (every file concatenated into main.jet, `use` lines hoisted and
// deduplicated). Both are gated: a compiler that only handles one file must
// fail the `files` series, not be excused from it.
export const LAYOUTS = Object.freeze(['files', 'single']);
export function corpus(shape, n, seed = 1, layout = 'files') {
  const entry = SHAPES[shape];
  if (!entry) throw new Error(`Unknown shape: ${shape} (shapes: ${Object.keys(SHAPES).join(', ')})`);
  if (!Number.isSafeInteger(n) || n < 1) throw new Error('N must be a positive safe integer');
  if (!LAYOUTS.includes(layout)) throw new Error(`Unknown layout: ${layout}`);
  const built = entry.build(n, random(seed));
  let files = built.files ?? built;
  if (layout === 'single') files = singleFile(files);
  const expect = built.expect ?? { diagnostics: 0 };
  return { shape, n, seed, layout, kind: entry.kind, files, expect, code_lines: codeLines(files), source_sha256: sourceDigest(files) };
}

// One package file: the other files in path order, then main.jet.
function singleFile(files) {
  const uses = new Set();
  const bodies = [];
  const names = Object.keys(files).filter(name => name !== 'package.jet' && name !== 'main.jet').sort();
  for (const name of [...names, 'main.jet']) {
    bodies.push(`// ---- ${name}`);
    for (const line of files[name].split('\n')) {
      if (/^use /.test(line)) uses.add(line);
      else bodies.push(line);
    }
  }
  return { 'package.jet': files['package.jet'], 'main.jet': `${[...uses, '', ...bodies].join('\n')}\n` };
}
