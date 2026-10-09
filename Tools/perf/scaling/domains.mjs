// Realistic domain corpora of the scaling gate (Tower #4601): web/backend
// services, games, CLI tools, data analysis, GUI applications, embedded,
// compilers/interpreters, scientific code, generic-heavy libraries and
// generated code. Each domain is a shared `common` file plus unit kinds; a
// unit is one self-contained cluster of types and functions (about 30-120
// lines) whose entry `fn <entry>(seed: Int) -> Int` the consumer tree calls,
// and whose leaf helpers `(x: Int) -> Int` later units call, so the program
// has a realistic cross-file call graph without one long chain. Units cycle
// through the domain's kinds until the package reaches the requested code
// lines, about 600 code lines per file.
//
// Adding a domain: write `common` and a list of kinds `(i, r, helper) =>
// { lines, entry, helpers }` (names carry `i`: one package is one
// namespace), then register it in DOMAINS. `helper(x)` returns a call into
// a random earlier unit's helper (or a constant expression for the first).
//
// Generic functions and records work only in the package's entry file today:
// outside it the Rust reference fails cost projection (E2104 "type-
// parameterized callable has no complete TIR specialization") and cannot
// unify a generic record across files (E0904/E0112), and the generated
// compiler loses the nominal identity (E0108) or the lowered type (E0003). A
// generic-heavy domain is therefore one entry file (`entryFile`), the way
// the compile-throughput cell's `mixed` program is one file.

import { consumerTree, finish, isCodeLine, MODULUS } from './package.mjs';

const M = MODULUS;
const WORDS = ['alpha', 'bravo', 'cedar', 'delta', 'ember', 'fjord', 'grove', 'harbor', 'island', 'juniper', 'kestrel', 'lantern'];
const codeOf = lines => lines.filter(isCodeLine).length;
const PER_FILE = 600;

// A decimal literal is a Decimal in Jet unless its context types it, and a
// Decimal never mixes with a Float, so every decimal literal outside a string
// is written `Float{...}`, except a comparison's right operand, which the
// comparison types (and where `Float{` would open the `if` body).
function floatLiterals(line) {
  const parts = line.split(/("(?:[^"\\]|\\.)*")/);
  return parts.map((part, index) => (index % 2 ? part
    : part.replace(/(?<![\w.{])(\d+\.\d+)(?![\w.])/g, (literal, _, offset) => (/(==|!=|<=|>=|<|(?<!-)>)\s*$/.test(part.slice(0, offset)) ? literal : `Float{${literal}}`)))).join('');
}

function build(name, common, kinds, targetLines, r, { entryFile = false } = {}) {
  const files = {};
  const entries = [];
  const helpers = [];
  const helper = x => (helpers.length ? `${r.pick(helpers)}(${x})` : `(${x}) % ${M}`);
  let code = codeOf(common) + 8;
  let file = { lines: [], code: 0 };
  let index = 0;
  const flush = () => {
    if (file.lines.length && !entryFile) files[`src/${name}_${index++}.jet`] = file.lines.join('\n');
    if (!entryFile) file = { lines: [], code: 0 };
  };
  for (let i = 0; code < targetLines; i++) {
    const unit = kinds[i % kinds.length](i, r, helper);
    file.lines.push(...unit.lines.map(floatLiterals), '');
    entries.push(unit.entry);
    helpers.push(...(unit.helpers ?? []));
    const size = codeOf(unit.lines);
    file.code += size;
    code += size + 1;
    if (file.code >= PER_FILE) flush();
  }
  flush();
  if (entryFile) files['main.jet'] = [...common.map(floatLiterals), ...file.lines].join('\n');
  else files['common.jet'] = common.map(floatLiterals).join('\n');
  return finish(name, files, consumerTree(files, entries, name));
}

// ------------------------------------------------------------------ web

const webCommon = [
  'struct Request {', '    method: String', '    path: String', '    user_id: Int', '    body: String', '}', '',
  'struct Response {', '    status: Int', '    body: String', '}', '',
  '#Error', 'enum APIError {', '    NotFound(String)', '    Invalid(field: String, reason: String)', '    Unauthorized', '}', '',
  'fn respond(status: Int, body: String) -> Response { Response{status: status, body: body} }', '',
  'fn api_error_status(error: APIError) -> Int {', '    if error == {', '        .NotFound(_) -> 404', '        .Invalid(_, _) -> 422', '        .Unauthorized -> 401', '    }', '}', '',
  'fn api_error_text(error: APIError) -> String {', '    if error == {', '        .NotFound(what) -> "missing {what}"', '        .Invalid(field, reason) -> "invalid {field}: {reason}"', '        .Unauthorized -> "unauthorized"', '    }', '}', '',
];

const web = [
  (i, r, helper) => {
    const T = `Customer${i}`;
    const a = r.int(2, 97);
    return {
      entry: `customer${i}_main`, helpers: [`customer${i}_score`],
      lines: [
        `struct ${T} {`, '    id: Int', '    name: String', '    email: String', '    tier: Int', '    active: Bool', '    tags: [String]', '}', '',
        `fn customer${i}_seed(count: Int) -> [${T}] {`, `    rows := [${T}]{}`, '    loop index in 0..<count {',
        `        &rows.push(${T}{id: index + 1, name: "customer-{index}", email: "c{index}@example.com", tier: index % ${r.int(2, 5)}, active: index % 3 != 0, tags: [String]{"t{index % 4}", "${r.pick(WORDS)}"}})`,
        '    }', '    rows', '}', '',
        `fn customer${i}_find(rows: [${T}], id: Int) -> ${T} APIError! {`, '    loop row in rows {', '        if row.id == id -> return Ok(row)', '    }',
        `    Err(APIError.NotFound("customer${i} {id}"))`, '}', '',
        `fn customer${i}_validate(row: ${T}) -> Bool APIError! {`,
        '    if row.name.len() == 0 -> return Err(APIError.Invalid{field: "name", reason: "empty"})',
        '    if row.email.len() < 5 -> return Err(APIError.Invalid{field: "email", reason: "too short"})', '    Ok(true)', '}', '',
        `fn customer${i}_to_json(row: ${T}) -> String {`,
        '    "{{\\"id\\": {row.id}, \\"name\\": \\"{row.name}\\", \\"email\\": \\"{row.email}\\", \\"tier\\": {row.tier}, \\"tags\\": {row.tags.len()}}}"', '}', '',
        `fn customer${i}_active_count(rows: [${T}]) -> Int { rows.filter((row) -> row.active && row.tier > 0).len() }`, '',
        `fn customer${i}_score(x: Int) -> Int { (x * ${a} + ${r.int(1, 99)}) % ${M} }`, '',
        `fn customer${i}_main(seed: Int) -> Int {`, `    rows :: customer${i}_seed(seed % 9 + 3)`,
        `    fallback :: ${T}{id: 0, name: "", email: "", tier: 0, active: false, tags: [String]{}}`,
        `    found :: customer${i}_find(rows, seed % 5 + 1) ?? fallback`,
        `    valid :: customer${i}_validate(found) ?? false`, '    bonus :: if valid -> 7 else -> 1',
        `    (customer${i}_to_json(found).len() + customer${i}_active_count(rows) + bonus + ${helper('seed')}) % ${M}`, '}',
      ],
    };
  },
  (i, r, helper) => ({
    entry: `handler${i}_main`, helpers: [],
    lines: [
      `fn lookup${i}(id: Int) -> String APIError! {`, '    if id <= 0 -> return Err(APIError.Unauthorized)',
      `    if id % ${r.int(5, 11)} == 0 -> return Err(APIError.NotFound("user {id}"))`, '    Ok("user-{id}")', '}', '',
      `fn items${i}_list(user: Int) -> String {`, '    parts := [String]{}', '    loop index in 0..<(user % 4 + 1) -> &parts.push("item-{index}")', '    parts.join(",")', '}', '',
      `fn handle${i}_item(request: Request) -> Response {`, `    if lookup${i}(request.user_id) == {`,
      '        .Ok(name) -> respond(200, "{{\\"name\\": \\"{name}\\"}}")', '        .Err(error) -> respond(api_error_status(error), api_error_text(error))', '    }', '}', '',
      `fn handle${i}(request: Request) -> Response {`, '    if request.method != "GET" && request.method != "POST" -> return respond(405, "method {request.method}")',
      '    if request.path == {', `        "/api/v${i}/items" -> respond(200, items${i}_list(request.user_id))`, `        "/api/v${i}/item" -> handle${i}_item(request)`,
      `        "/api/v${i}/health" -> respond(200, "ok")`, '        else -> respond(404, "no route {request.path}")', '    }', '}', '',
      `fn handler${i}_main(seed: Int) -> Int {`, `    paths :: [String]{"/api/v${i}/items", "/api/v${i}/item", "/api/v${i}/health", "/missing"}`, '    total := 0',
      '    loop (index, path) in paths {', `        response :: handle${i}(Request{method: "GET", path: path, user_id: seed % 11 + index, body: ""})`,
      `        total = (total + response.status + response.body.len()) % ${M}`, '    }', `    (total + ${helper('total')}) % ${M}`, '}',
    ],
  }),
  (i, r, helper) => ({
    entry: `template${i}_main`, helpers: [`template${i}_width`],
    lines: [
      `fn render${i}_row(cells: [String]) -> String {`, '    out := "<tr>"', '    loop cell in cells -> out = "{out}<td>{cell}</td>"', '    "{out}</tr>"', '}', '',
      `fn render${i}_page(title: String, rows: [[String]]) -> String {`, '    body := ""', `    loop row in rows -> body = "{body}{render${i}_row(row)}"`,
      '    "<html><head><title>{title}</title></head><body><h1>{title}</h1><table>{body}</table></body></html>"', '}', '',
      `fn template${i}_width(x: Int) -> Int { (x % ${r.int(40, 120)}) + ${r.int(1, 9)} }`, '',
      `fn template${i}_main(seed: Int) -> Int {`, '    rows := [[String]]{}',
      `    loop index in 0..<(seed % 5 + 2) -> &rows.push([String]{"r{index}", "{index * seed % 97}", "${r.pick(WORDS)}"})`,
      `    page :: render${i}_page("Report ${i}", rows)`, `    (page.len() + ${helper('seed')}) % ${M}`, '}',
    ],
  }),
  (i, r, helper) => ({
    entry: `query${i}_main`, helpers: [],
    lines: [
      `struct Query${i} {`, '    table: String', '    filters: [String]', '    limit: Int', '    order: String', '}', '',
      `impl Query${i} {`, '    fn where_eq(&self, column: String, value: Int) { &self.filters.push("{column} = {value}") }', '',
      '    fn order_by(&self, column: String) { self.order = column }', '',
      '    fn to_sql(self) -> String {', '        clause := ""', '        loop (index, filter) in self.filters {',
      '            clause = if index == 0 -> "WHERE {filter}" else -> "{clause} AND {filter}"', '        }',
      '        "SELECT * FROM {self.table} {clause} ORDER BY {self.order} LIMIT {self.limit}"', '    }', '}', '',
      `fn query${i}_main(seed: Int) -> Int {`, `    query := Query${i}{table: "${r.pick(WORDS)}_${i}", filters: [String]{}, limit: seed % 50 + 1, order: "id"}`,
      '    &query.where_eq("tenant", seed % 13)', `    &query.where_eq("status", ${r.int(0, 4)})`, '    &query.order_by("created_at")',
      `    (query.to_sql().len() + ${helper('seed + 3')}) % ${M}`, '}',
    ],
  }),
  (i, r, helper) => ({
    entry: `permissions${i}_main`, helpers: [`permissions${i}_level`],
    lines: [
      `enum Role${i} {`, '    Guest', '    Member(Int)', '    Admin(String)', '}', '',
      `fn role${i}_of(id: Int) -> Role${i} {`, '    if id % 3 == {', `        0 -> Role${i}.Guest`, `        1 -> Role${i}.Member(id)`, `        else -> Role${i}.Admin("root")`, '    }', '}', '',
      `fn can${i}(role: Role${i}, action: String) -> Bool {`, '    if role == {', '        .Guest -> action == "read"', `        .Member(level) -> action != "delete" || level > ${r.int(50, 150)}`, '        .Admin(_) -> true', '    }', '}', '',
      `fn permissions${i}_level(x: Int) -> Int { (x * ${r.int(2, 9)}) % 101 }`, '',
      `fn permissions${i}_main(seed: Int) -> Int {`, '    actions :: [String]{"read", "write", "delete"}', '    allowed := 0',
      '    loop index in 0..<6 {', `        role :: role${i}_of(seed + index)`, '        loop action in actions {', `            if can${i}(role, action) -> allowed += 1`, '        }', '    }',
      `    (allowed + ${helper('allowed')}) % ${M}`, '}',
    ],
  }),
];

// ----------------------------------------------------------------- game

const gameCommon = [
  'struct Vec2 {', '    x: Float', '    y: Float', '}', '',
  'fn vec2(x: Float, y: Float) -> Vec2 { Vec2{x: x, y: y} }', '',
  'fn vec_add(a: Vec2, b: Vec2) -> Vec2 { Vec2{x: a.x + b.x, y: a.y + b.y} }', '',
  'fn vec_scale(a: Vec2, k: Float) -> Vec2 { Vec2{x: a.x * k, y: a.y * k} }', '',
  'fn vec_dot(a: Vec2, b: Vec2) -> Float { a.x * b.x + a.y * b.y }', '',
  'fn clampf(v: Float, lo: Float, hi: Float) -> Float {', '    if {', '        v < lo -> lo', '        v > hi -> hi', '        else -> v', '    }', '}', '',
  'fn to_score(v: Float) -> Int { (Int.from_float(clampf(v, 0.0, 1000000.0)) ?? 0) % 1000003 }', '',
  'trait Behavior {', '    fn tick(self, dt: Float) -> Float', '    fn label(self) -> String', '}', '',
];

const game = [
  (i, r, helper) => ({
    entry: `system${i}_main`, helpers: [`system${i}_frames`],
    lines: [
      `struct Body${i} {`, '    position: Vec2', '    velocity: Vec2', '    mass: Float', '    alive: Bool', '}', '',
      `fn bodies${i}_spawn(count: Int) -> [Body${i}] {`, `    bodies := [Body${i}]{}`, '    loop index in 0..<count {',
      `        &bodies.push(Body${i}{position: vec2(Float.from_int(index), 0.5), velocity: vec2(1.0, Float.from_int(index % 3)), mass: 1.0 + Float.from_int(index % 5), alive: true})`,
      '    }', '    bodies', '}', '',
      `fn bodies${i}_step(bodies: [Body${i}], dt: Float) -> [Body${i}] {`, `    out := [Body${i}]{}`, '    loop body in bodies {',
      `        gravity :: vec2(0.0, 0.0 - ${r.int(5, 12)}.8 * dt)`, '        velocity :: vec_add(body.velocity, gravity)', '        position :: vec_add(body.position, vec_scale(velocity, dt))',
      `        &out.push(Body${i}{position: position, velocity: velocity, mass: body.mass, alive: position.y + 50.0 > 0.0})`, '    }', '    out', '}', '',
      `fn bodies${i}_energy(bodies: [Body${i}]) -> Float {`, '    total := 0.0', '    loop body in bodies {', '        if body.alive -> total = total + 0.5 * body.mass * vec_dot(body.velocity, body.velocity)', '    }', '    total', '}', '',
      `fn system${i}_frames(x: Int) -> Int { x % ${r.int(3, 9)} + 1 }`, '',
      `fn system${i}_main(seed: Int) -> Int {`, `    bodies := bodies${i}_spawn(seed % 6 + 2)`,
      `    loop frame in 0..<system${i}_frames(seed) -> bodies = bodies${i}_step(bodies, 0.016 + Float.from_int(frame) * 0.001)`,
      `    (to_score(bodies${i}_energy(bodies)) + ${helper('seed')}) % ${M}`, '}',
    ],
  }),
  (i, r, helper) => ({
    entry: `ai${i}_main`, helpers: [],
    lines: [
      `enum Mind${i} {`, '    Idle', '    Patrol(Int)', '    Chase(target: Int, speed: Float)', '    Flee(Float)', '}', '',
      `fn mind${i}_next(state: Mind${i}, distance: Int, health: Int) -> Mind${i} {`, '    if state == {', '        .Idle -> {',
      `            if distance < ${r.int(5, 20)} -> return Mind${i}.Chase{target: distance, speed: 1.5}`, `            Mind${i}.Patrol(distance % 4)`, '        }',
      '        .Patrol(point) -> {', `            if health < 20 -> return Mind${i}.Flee(2.0)`, `            Mind${i}.Patrol((point + 1) % 4)`, '        }',
      '        .Chase(target, speed) -> {', `            if health < 30 -> return Mind${i}.Flee(speed)`, `            Mind${i}.Chase{target: target - 1, speed: speed}`, '        }',
      '        .Flee(speed) -> {', `            if health > 60 -> return Mind${i}.Idle`, `            Mind${i}.Flee(speed * 0.9)`, '        }', '    }', '}', '',
      `fn mind${i}_code(state: Mind${i}) -> Int {`, '    if state == {', '        .Idle -> 0', '        .Patrol(point) -> 10 + point', '        .Chase(target, _) -> 100 + target', '        .Flee(_) -> 3', '    }', '}', '',
      `fn ai${i}_main(seed: Int) -> Int {`, `    state := Mind${i}.Idle`, '    total := 0', '    loop tick in 0..<8 {',
      `        state = mind${i}_next(state, (seed + tick * ${r.int(3, 17)}) % 40, (seed * tick) % 100)`, `        total = (total + mind${i}_code(state)) % ${M}`, '    }',
      `    (total + ${helper('total + 1')}) % ${M}`, '}',
    ],
  }),
  (i, r, helper) => {
    const values = Array.from({ length: 64 }, () => r.int(0, 999));
    return {
      entry: `waves${i}_main`, helpers: [`wave${i}_pick`],
      lines: [
        `fn wave${i}_table() -> [Int] {`, '    [Int]{', ...[0, 16, 32, 48].map(start => `        ${values.slice(start, start + 16).join(', ')},`), '    }', '}', '',
        `struct Spawn${i} {`, '    kind: Int', '    count: Int', '    delay: Float', '}', '',
        `fn wave${i}_spawns(level: Int) -> [Spawn${i}] {`, `    table :: wave${i}_table()`, `    spawns := [Spawn${i}]{}`, '    loop index in 0..<8 {',
        '        value :: table[(level * 8 + index) % table.len()]', `        &spawns.push(Spawn${i}{kind: value % 5, count: value % 9 + 1, delay: Float.from_int(value % 30) * 0.1})`, '    }', '    spawns', '}', '',
        `fn wave${i}_pick(x: Int) -> Int { wave${i}_table()[x % 64] }`, '',
        `fn waves${i}_main(seed: Int) -> Int {`, '    total := 0', `    loop spawn in wave${i}_spawns(seed % 7) {`,
        `        total = (total + spawn.kind * spawn.count + to_score(spawn.delay)) % ${M}`, '    }', `    (total + ${helper('seed')}) % ${M}`, '}',
      ],
    };
  },
  (i, r, helper) => ({
    entry: `inventory${i}_main`, helpers: [],
    lines: [
      `struct Item${i} {`, '    name: String', '    weight: Int', '    value: Int', '    rarity: Int', '}', '',
      `fn inventory${i}_main(seed: Int) -> Int {`, `    items := [Item${i}]{}`,
      `    loop index in 0..<(seed % 7 + 3) -> &items.push(Item${i}{name: "${r.pick(WORDS)}-{index}", weight: index * ${r.int(2, 9)} % 23, value: index * 17 % 101, rarity: index % 5})`,
      `    heavy :: items.filter((item) -> item.weight > ${r.int(5, 15)}).len()`,
      '    best := items[0]',
      `    loop item in items {`, '        if item.rarity > best.rarity -> best = item', '    }',
      `    (heavy + best.value + best.name.len() + ${helper('heavy')}) % ${M}`, '}',
    ],
  }),
  (i, r, helper) => ({
    entry: `behaviors${i}_main`, helpers: [],
    lines: [
      `struct Turret${i} {`, '    range: Float', '    rate: Float', '    heat: Float', '}', '',
      `impl Turret${i}.Behavior {`, '    fn tick(self, dt: Float) -> Float { self.heat + self.rate * dt + self.range * 0.01 }', '', `    fn label(self) -> String { "turret${i}" }`, '}', '',
      `fn behaviors${i}_main(seed: Int) -> Int {`,
      `    agents :: [Behavior]{Turret${i}{range: ${r.int(5, 30)}.0, rate: 2.0, heat: 0.0}, Turret${i}{range: Float.from_int(seed % 20), rate: 1.0, heat: 0.5}}`,
      '    total := 0.0', '    loop agent in agents -> total = total + agent.tick(0.016) + Float.from_int(agent.label().len())',
      `    (to_score(total) + ${helper('seed')}) % ${M}`, '}',
    ],
  }),
];

// ------------------------------------------------------------------ cli

const cliCommon = [
  '#Error', 'enum CLIError {', '    UnknownFlag(String)', '    MissingValue(String)', '    BadNumber(String)', '}', '',
  'fn cli_error_text(error: CLIError) -> String {', '    if error == {', '        .UnknownFlag(flag) -> "unknown flag {flag}"', '        .MissingValue(flag) -> "{flag} needs a value"', '        .BadNumber(text) -> "not a number: {text}"', '    }', '}', '',
  'struct Options {', '    verbose: Bool', '    count: Int', '    name: String', '    extra: [String]', '}', '',
  'fn default_options() -> Options { Options{verbose: false, count: 1, name: "default", extra: [String]{}} }', '',
  'fn pad_right(text: String, width: Int) -> String {', '    out := text', '    loop out.len() < width -> out = "{out} "', '    out', '}', '',
];

const cli = [
  (i, r, helper) => ({
    entry: `parse${i}_main`, helpers: [],
    lines: [
      `fn parse${i}_args(args: [String]) -> Options CLIError! {`, '    options := default_options()', '    index := 0', '    loop index < args.len() {', '        arg :: args[index]', '        if arg == {',
      '            "--verbose" -> options.verbose = true', '            "--count" -> {', '                if index + 1 >= args.len() -> return Err(CLIError.MissingValue(arg))',
      '                options.count = args[index + 1].to_int() ?? 0', '                index += 1', '            }',
      '            "--name" -> {', '                if index + 1 >= args.len() -> return Err(CLIError.MissingValue(arg))', '                options.name = args[index + 1]', '                index += 1', '            }',
      `            "--${r.pick(WORDS)}" -> return Err(CLIError.UnknownFlag(arg))`, '            else -> &options.extra.push(arg)', '        }', '        index += 1', '    }', '    Ok(options)', '}', '',
      `fn parse${i}_main(seed: Int) -> Int {`, `    args :: [String]{"--verbose", "--count", "{seed % 9}", "--name", "job${i}", "input-{seed}.txt"}`,
      `    if parse${i}_args(args) == {`, `        .Ok(options) -> (options.count + options.name.len() + options.extra.len() + ${helper('seed')}) % ${M}`,
      '        .Err(error) -> cli_error_text(error).len()', '    }', '}',
    ],
  }),
  (i, r, helper) => ({
    entry: `command${i}_main`, helpers: [`command${i}_weight`],
    lines: [
      `enum Command${i} {`, '    Build(String)', '    Test(Int)', '    Clean', '    Help', '}', '',
      `fn command${i}_parse(word: String, arg: Int) -> Command${i} {`, '    if word == {', `        "build" -> Command${i}.Build("target-{arg}")`, `        "test" -> Command${i}.Test(arg)`, `        "clean" -> Command${i}.Clean`, `        else -> Command${i}.Help`, '    }', '}', '',
      `fn command${i}_run(command: Command${i}) -> [String] {`, '    if command == {', '        .Build(target) -> [String]{"compiling {target}", "linking {target}"}',
      '        .Test(count) -> [String]{"running {count} tests", "ok"}', '        .Clean -> [String]{"removed build"}', `        .Help -> [String]{"usage: tool${i} <command>"}`, '    }', '}', '',
      `fn command${i}_weight(x: Int) -> Int { (x * ${r.int(2, 13)} + ${r.int(1, 9)}) % ${M} }`, '',
      `fn command${i}_main(seed: Int) -> Int {`, '    words :: [String]{"build", "test", "clean", "help"}', '    total := 0', '    loop word in words {',
      `        loop line in command${i}_run(command${i}_parse(word, seed % 5)) -> total = (total + pad_right(line, ${r.int(20, 40)}).len()) % ${M}`, '    }',
      `    (total + ${helper('total')}) % ${M}`, '}',
    ],
  }),
  (i, r, helper) => ({
    entry: `table${i}_main`, helpers: [],
    lines: [
      `struct Column${i} {`, '    title: String', '    width: Int', '}', '',
      `fn table${i}_render(columns: [Column${i}], rows: [[String]]) -> String {`, '    header := ""', '    loop column in columns -> header = "{header}{pad_right(column.title, column.width)}|"', '    out := header',
      '    loop row in rows {', '        line := ""', '        loop (index, cell) in row {', '            width :: if index < columns.len() -> columns[index].width else -> 8', '            line = "{line}{pad_right(cell, width)}|"', '        }',
      '        out = "{out}\\n{line}"', '    }', '    out', '}', '',
      `fn table${i}_main(seed: Int) -> Int {`, `    columns :: [Column${i}]{Column${i}{title: "name", width: ${r.int(8, 16)}}, Column${i}{title: "size", width: 8}, Column${i}{title: "${r.pick(WORDS)}", width: 10}}`,
      '    rows := [[String]]{}', '    loop index in 0..<(seed % 4 + 2) -> &rows.push([String]{"file{index}", "{index * 1024}", "{seed % 7}"})',
      `    (table${i}_render(columns, rows).len() + ${helper('seed')}) % ${M}`, '}',
    ],
  }),
  (i, r, helper) => ({
    entry: `config${i}_main`, helpers: [`config${i}_default`],
    lines: [
      `fn config${i}_read(settings: [String:String], key: String, fallback: Int) -> Int {`, '    text :: settings.get(key) ?? ""', '    if text == "" -> return fallback', '    text.to_int() ?? fallback', '}', '',
      `fn config${i}_default(x: Int) -> Int { x % ${r.int(10, 60)} + 1 }`, '',
      `fn config${i}_main(seed: Int) -> Int {`, '    settings := [String:String]{}', '    settings["jobs"] = "{seed % 16 + 1}"', `    settings["retries"] = "${r.int(1, 5)}"`, '    settings["mode"] = "fast"',
      `    jobs :: config${i}_read(settings, "jobs", 1)`, `    retries :: config${i}_read(settings, "retries", 3)`, `    timeout :: config${i}_read(settings, "timeout", config${i}_default(seed))`,
      `    (jobs * retries + timeout + ${helper('jobs')}) % ${M}`, '}',
    ],
  }),
];

// ----------------------------------------------------------------- data

const dataCommon = [
  'fn mean(values: [Float]) -> Float {', '    if values.len() == 0 -> return 0.0', '    total := 0.0', '    loop value in values -> total = total + value', '    total / Float.from_int(values.len())', '}', '',
  'fn variance(values: [Float]) -> Float {', '    center :: mean(values)', '    total := 0.0', '    loop value in values -> total = total + (value - center) * (value - center)', '    if values.len() < 2 -> return 0.0', '    total / Float.from_int(values.len() - 1)', '}', '',
  'fn to_score(v: Float) -> Int {', '    if v < 0.0 || v > 1000000.0 -> return 0', '    (Int.from_float(v) ?? 0) % 1000003', '}', '',
];

const data = [
  (i, r, helper) => ({
    entry: `column${i}_main`, helpers: [],
    lines: [
      `fn column${i}_build(seed: Int, count: Int) -> [Float] {`, '    values := [Float]{}', `    loop index in 0..<count -> &values.push(Float.from_int((seed * ${r.int(3, 31)} + index * ${r.int(3, 17)}) % 1000) * 0.1)`, '    values', '}', '',
      `fn column${i}_zscores(values: [Float]) -> [Float] {`, '    center :: mean(values)', '    spread :: variance(values) + 1.0', '    values.map((value) -> (value - center) / spread)', '}', '',
      `fn column${i}_main(seed: Int) -> Int {`, `    values :: column${i}_build(seed, ${r.int(8, 32)})`, `    scores :: column${i}_zscores(values)`,
      '    high :: scores.filter((score) -> score > 0.0).len()', `    (to_score(mean(values) * 10.0) + high + ${helper('seed')}) % ${M}`, '}',
    ],
  }),
  (i, r, helper) => ({
    entry: `groups${i}_main`, helpers: [`groups${i}_bucket`],
    lines: [
      `struct Sale${i} {`, '    region: String', '    product: Int', '    amount: Float', '}', '',
      `fn groups${i}_bucket(x: Int) -> Int { x % ${r.int(3, 9)} }`, '',
      `fn groups${i}_sum(rows: [Sale${i}]) -> [String:Float] {`, '    sums := [String:Float]{}', '    loop row in rows -> sums[row.region] = (sums.get(row.region) ?? 0.0) + row.amount', '    sums', '}', '',
      `fn groups${i}_main(seed: Int) -> Int {`, `    regions :: [String]{"north", "south", "east", "${r.pick(WORDS)}"}`, `    rows := [Sale${i}]{}`,
      `    loop index in 0..<(seed % 12 + 4) -> &rows.push(Sale${i}{region: regions[groups${i}_bucket(index) % 4], product: index % 7, amount: Float.from_int(index * 13 % 97) + 0.5})`,
      '    total := 0', `    loop (region, amount) in groups${i}_sum(rows) -> total = (total + region.len() + to_score(amount)) % ${M}`,
      `    (total + ${helper('total')}) % ${M}`, '}',
    ],
  }),
  (i, r, helper) => ({
    entry: `pipeline${i}_main`, helpers: [],
    lines: [
      `struct Reading${i} {`, '    sensor: Int', '    value: Float', '    valid: Bool', '}', '',
      `fn pipeline${i}_main(seed: Int) -> Int {`, `    readings := [Reading${i}]{}`,
      `    loop index in 0..<(seed % 10 + 5) -> &readings.push(Reading${i}{sensor: index % 3, value: Float.from_int(index * ${r.int(3, 19)} % 50), valid: index % 4 != 0})`,
      '    good :: readings.filter((reading) -> reading.valid && reading.value > 5.0)', `    scaled :: good.map((reading) -> reading.value * ${r.int(1, 3)}.5)`,
      `    (to_score(mean(scaled)) + good.len() + ${helper('seed')}) % ${M}`, '}',
    ],
  }),
  (i, r, helper) => ({
    entry: `csv${i}_main`, helpers: [`csv${i}_width`],
    lines: [
      '#Error', `enum RowError${i} {`, '    Short(Int)', '    BadField(String)', '}', '',
      `fn csv${i}_parse(line: String) -> Int RowError${i}! {`, '    fields :: line.split(",").to_list()', `    if fields.len() < 3 -> return Err(RowError${i}.Short(fields.len()))`,
      '    count :: fields[1].to_int() ?? -1', `    if count < 0 -> return Err(RowError${i}.BadField(fields[1]))`, '    Ok(count + fields[2].len())', '}', '',
      `fn csv${i}_width(x: Int) -> Int { x % ${r.int(5, 50)} }`, '',
      `fn csv${i}_main(seed: Int) -> Int {`, `    lines :: [String]{"a,{seed % 10},x", "b,oops,y", "c,{seed % 3}", "d,${r.int(1, 99)},${r.pick(WORDS)}"}`, '    total := 0',
      '    loop line in lines {', `        total = if csv${i}_parse(line) == {`, '            .Ok(value) -> total + value', '            .Err(_) -> total + 1', '        }', '    }',
      `    (total + ${helper('total')}) % ${M}`, '}',
    ],
  }),
];

// ------------------------------------------------------------------ gui

const guiCommon = [
  'struct Rect {', '    x: Int', '    y: Int', '    w: Int', '    h: Int', '}', '',
  'fn rect_area(rect: Rect) -> Int { rect.w * rect.h }', '',
  'fn rect_contains(rect: Rect, px: Int, py: Int) -> Bool { px >= rect.x && py >= rect.y && px < rect.x + rect.w && py < rect.y + rect.h }', '',
];

const gui = [
  (i, r, helper) => ({
    entry: `layout${i}_main`, helpers: [],
    lines: [
      `enum Widget${i} {`, '    Label(String)', '    Button(text: String, id: Int)', '    Spacer(Int)', '    Image(width: Int, height: Int)', '}', '',
      `fn widget${i}_height(widget: Widget${i}) -> Int {`, '    if widget == {', '        .Label(text) -> 12 + text.len() /% 40 * 12', '        .Button(_, _) -> 28', '        .Spacer(size) -> size', '        .Image(_, height) -> height', '    }', '}', '',
      `fn layout${i}_column(widgets: [Widget${i}], width: Int) -> [Rect] {`, '    rects := [Rect]{}', '    y := 0', '    loop widget in widgets {', `        height :: widget${i}_height(widget)`,
      '        &rects.push(Rect{x: 0, y: y, w: width, h: height})', `        y += height + ${r.int(2, 8)}`, '    }', '    rects', '}', '',
      `fn layout${i}_main(seed: Int) -> Int {`, `    widgets :: [Widget${i}]{Widget${i}.Label("${r.pick(WORDS)} panel"), Widget${i}.Button{text: "ok", id: seed % 9}, Widget${i}.Spacer(8), Widget${i}.Image{width: 64, height: seed % 48 + 16}}`,
      `    rects :: layout${i}_column(widgets, ${r.int(120, 480)})`, '    total := 0', '    loop rect in rects -> total += rect_area(rect) % 997',
      `    (total + ${helper('total')}) % ${M}`, '}',
    ],
  }),
  (i, r, helper) => ({
    entry: `reducer${i}_main`, helpers: [`reducer${i}_step`],
    lines: [
      `struct State${i} {`, '    count: Int', '    title: String', '    items: [String]', '    selected: Int', '}', '',
      `enum Msg${i} {`, '    Increment(Int)', '    Rename(String)', '    Add(String)', '    Select(Int)', '    Reset', '}', '',
      `fn update${i}(state: State${i}, msg: Msg${i}) -> State${i} {`, '    updated := state', '    if msg == {', '        .Increment(by) -> updated.count = updated.count + by', '        .Rename(title) -> updated.title = title',
      '        .Add(item) -> &updated.items.push(item)', '        .Select(index) -> updated.selected = index', `        .Reset -> updated = State${i}{count: 0, title: "", items: [String]{}, selected: -1}`, '    }', '    updated', '}', '',
      `fn reducer${i}_step(x: Int) -> Int { (x + ${r.int(1, 9)}) % ${r.int(10, 30)} }`, '',
      `fn reducer${i}_main(seed: Int) -> Int {`, `    state := State${i}{count: 0, title: "start", items: [String]{}, selected: -1}`,
      `    messages :: [Msg${i}]{Msg${i}.Increment(seed % 5), Msg${i}.Add("a"), Msg${i}.Rename("${r.pick(WORDS)}"), Msg${i}.Select(1), Msg${i}.Add("b"), Msg${i}.Increment(reducer${i}_step(seed))}`,
      `    loop msg in messages -> state = update${i}(state, msg)`, `    (state.count + state.title.len() + state.items.len() + state.selected + ${helper('seed')}) % ${M}`, '}',
    ],
  }),
  (i, r, helper) => ({
    entry: `style${i}_main`, helpers: [],
    lines: [
      `struct Style${i} {`, ...['margin', 'padding', 'border', 'radius', 'font_size', 'line_height', 'weight', 'opacity', 'shadow', 'gap', 'min_width', 'max_width'].map(f => `    ${f}: Int`), '    color: String', '    background: String', '}', '',
      `fn style${i}_default() -> Style${i} { Style${i}{margin: 4, padding: 8, border: 1, radius: 4, font_size: 14, line_height: 20, weight: 400, opacity: 100, shadow: 0, gap: 8, min_width: 0, max_width: 1200, color: "#222", background: "#fff"} }`, '',
      `fn style${i}_emphasis(base: Style${i}) -> Style${i} {`, '    styled := base', `    styled.weight = ${r.pick([600, 700, 800])}`, '    styled.font_size = base.font_size + 2', '    styled.color = "#000"', '    styled', '}', '',
      `fn style${i}_css(style: Style${i}) -> String { "margin:{style.margin}px;padding:{style.padding}px;border:{style.border}px;radius:{style.radius}px;font-size:{style.font_size}px;weight:{style.weight};color:{style.color};background:{style.background}" }`, '',
      `fn style${i}_main(seed: Int) -> Int {`, `    css :: style${i}_css(style${i}_emphasis(style${i}_default()))`, `    (css.len() + seed % 7 + ${helper('seed')}) % ${M}`, '}',
    ],
  }),
  (i, r, helper) => ({
    entry: `events${i}_main`, helpers: [],
    lines: [
      `enum Event${i} {`, '    Click(x: Int, y: Int)', '    Key(String)', '    Scroll(Int)', '    Resize(w: Int, h: Int)', '}', '',
      `fn events${i}_handle(event: Event${i}, area: Rect) -> Int {`, '    if event == {', '        .Click(x, y) -> if rect_contains(area, x, y) -> 1 else -> 0',
      '        .Key(name) -> if name == "Enter" -> 2 else -> name.len()', '        .Scroll(delta) -> delta % 5', '        .Resize(w, h) -> (w * h) % 11', '    }', '}', '',
      `fn events${i}_main(seed: Int) -> Int {`, `    area :: Rect{x: 10, y: 10, w: ${r.int(50, 200)}, h: 40}`,
      `    events :: [Event${i}]{Event${i}.Click{x: seed % 100, y: 20}, Event${i}.Key("Enter"), Event${i}.Scroll(seed % 9), Event${i}.Resize{w: 800, h: 600}}`,
      '    handled := 0', `    loop event in events -> handled += events${i}_handle(event, area)`, `    (handled + ${helper('handled')}) % ${M}`, '}',
    ],
  }),
];

// ------------------------------------------------------------- embedded

const embeddedCommon = [
  'fn set_bit(value: Int, bit: Int) -> Int { value.bit_or(1 << bit) }', '',
  'fn clear_bit(value: Int, bit: Int) -> Int { value.bit_and(65535 - (1 << bit)) }', '',
  'fn read_field(value: Int, shift: Int, mask: Int) -> Int { (value >> shift).bit_and(mask) }', '',
];

const embedded = [
  (i, r, helper) => ({
    entry: `regs${i}_main`, helpers: [`regs${i}_mask`],
    lines: [
      `REG${i}_CTRL :: ${r.int(16, 255)}`, `REG${i}_STATUS :: ${r.int(16, 255)}`, `REG${i}_DATA :: ${r.int(256, 4095)}`, '',
      `fn regs${i}_mask(x: Int) -> Int { x.bit_and(${r.int(15, 255)}) }`, '',
      `fn regs${i}_configure(ctrl: Int, speed: Int) -> Int {`, '    value := set_bit(ctrl, 0)', '    value = clear_bit(value, 3)', '    value = value.bit_or(speed.bit_and(7) << 4)', '    value.bit_and(65535)', '}', '',
      `fn regs${i}_main(seed: Int) -> Int {`, `    ctrl :: regs${i}_configure(REG${i}_CTRL, seed % 8)`, `    ready :: read_field(REG${i}_STATUS, 2, 3)`,
      `    (ctrl + ready + regs${i}_mask(REG${i}_DATA + seed) + ${helper('ctrl')}) % ${M}`, '}',
    ],
  }),
  (i, r, helper) => ({
    entry: `ring${i}_main`, helpers: [],
    lines: [
      `struct Ring${i} {`, '    data: [Int#16]', '    head: Int', '    count: Int', '}', '',
      `impl Ring${i} {`, '    fn push(&self, value: Int) {', '        self.data[(self.head + self.count) % 16] = value', '        if self.count < 16 -> self.count += 1 else -> self.head = (self.head + 1) % 16', '    }', '',
      '    fn sum(self) -> Int {', '        total := 0', '        loop k in 0..<self.count -> total += self.data[(self.head + k) % 16]', '        total', '    }', '}', '',
      `fn ring${i}_main(seed: Int) -> Int {`, `    ring := Ring${i}{data: [Int#16]{0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0}, head: 0, count: 0}`,
      `    loop k in 0..<${r.int(10, 30)} -> &ring.push((seed + k * ${r.int(3, 11)}) % 256)`, `    (ring.sum() + ${helper('seed')}) % ${M}`, '}',
    ],
  }),
  (i, r, helper) => {
    const table = Array.from({ length: 64 }, () => r.int(0, 65535));
    return {
      entry: `crc${i}_main`, helpers: [`crc${i}_byte`],
      lines: [
        `fn crc${i}_table() -> [Int] {`, '    [Int]{', ...[0, 16, 32, 48].map(start => `        ${table.slice(start, start + 16).join(', ')},`), '    }', '}', '',
        `fn crc${i}_byte(x: Int) -> Int { crc${i}_table()[x.bit_and(63)] }`, '',
        `fn crc${i}_update(crc: Int, bytes: [Int]) -> Int {`, '    value := crc', `    table :: crc${i}_table()`, '    loop byte in bytes -> value = ((value >> 4) + table[(value + byte).bit_and(63)]).bit_and(65535)', '    value', '}', '',
        `fn crc${i}_main(seed: Int) -> Int {`, '    bytes := [Int]{}', '    loop k in 0..<12 -> &bytes.push((seed * 7 + k).bit_and(255))', `    (crc${i}_update(${r.int(0, 65535)}, bytes) + ${helper('seed')}) % ${M}`, '}',
      ],
    };
  },
  (i, r, helper) => ({
    entry: `frame${i}_main`, helpers: [],
    lines: [
      `enum Frame${i} {`, '    Sync', '    Header(Int)', '    Payload(remaining: Int, sum: Int)', '    Done(Int)', '}', '',
      `fn frame${i}_step(state: Frame${i}, byte: Int) -> Frame${i} {`, '    if state == {', `        .Sync -> if byte == ${r.int(120, 130)} -> Frame${i}.Header(0) else -> Frame${i}.Sync`,
      `        .Header(_) -> Frame${i}.Payload{remaining: byte.bit_and(7), sum: 0}`, `        .Payload(remaining, sum) -> if remaining <= 1 -> Frame${i}.Done(sum + byte) else -> Frame${i}.Payload{remaining: remaining - 1, sum: sum + byte}`,
      '        .Done(sum) -> if byte == 0 -> state else -> Frame' + i + '.Sync', '    }', '}', '',
      `fn frame${i}_main(seed: Int) -> Int {`, `    state := Frame${i}.Sync`, '    done := 0', '    loop k in 0..<24 {', `        state = frame${i}_step(state, (seed + k * 31).bit_and(255))`,
      '        if state == .Done(sum) -> done += sum', '    }', `    (done + ${helper('done')}) % ${M}`, '}',
    ],
  }),
];

// ------------------------------------------------------------- compiler

const compilerCommon = [
  'struct Symbol {', '    name: String', '    slot: Int', '    depth: Int', '}', '',
  'fn symbol_lookup(table: [String:Int], name: String) -> Int { table.get(name) ?? -1 }', '',
];

const compiler = [
  (i, r, helper) => {
    const kinds = ['Num(Int)', 'Ident(String)', 'Plus', 'Minus', 'Star', 'Slash', 'LParen', 'RParen', 'Comma', 'Let', 'Assign', 'End'];
    return {
      entry: `lexer${i}_main`, helpers: [],
      lines: [
        `enum Tok${i} {`, ...kinds.map(k => `    ${k}`), '}', '',
        `fn lex${i}(codes: [Int]) -> [Tok${i}] {`, `    tokens := [Tok${i}]{}`, '    loop code in codes {', '        token :: if code % 12 == {',
        `            0 -> Tok${i}.Num(code)`, `            1 -> Tok${i}.Ident("v{code}")`, `            2 -> Tok${i}.Plus`, `            3 -> Tok${i}.Minus`, `            4 -> Tok${i}.Star`, `            5 -> Tok${i}.Slash`,
        `            6 -> Tok${i}.LParen`, `            7 -> Tok${i}.RParen`, `            8 -> Tok${i}.Comma`, `            9 -> Tok${i}.Let`, `            10 -> Tok${i}.Assign`, `            else -> Tok${i}.End`, '        }', '        &tokens.push(token)', '    }', '    tokens', '}', '',
        `fn tok${i}_weight(token: Tok${i}) -> Int {`, '    if token == {', '        .Num(value) -> value % 10', '        .Ident(name) -> name.len()', '        .Plus -> 1', '        .Minus -> 2', '        .Star -> 3', '        .Slash -> 4',
        '        .LParen -> 5', '        .RParen -> 6', '        .Comma -> 7', '        .Let -> 8', '        .Assign -> 9', '        .End -> 0', '    }', '}', '',
        `fn lexer${i}_main(seed: Int) -> Int {`, '    codes := [Int]{}', `    loop k in 0..<${r.int(16, 40)} -> &codes.push((seed * 7 + k * ${r.int(3, 13)}) % 97)`, '    total := 0',
        `    loop token in lex${i}(codes) -> total = (total * 3 + tok${i}_weight(token)) % ${M}`, `    (total + ${helper('total')}) % ${M}`, '}',
      ],
    };
  },
  (i, r, helper) => ({
    entry: `eval${i}_main`, helpers: [`eval${i}_fold`],
    lines: [
      `enum Node${i} {`, '    Lit(Int)', '    Var(String)', '    Add(left: Int, right: Int)', '    Mul(left: Int, right: Int)', '    Neg(Int)', '}', '',
      `fn eval${i}(nodes: [Node${i}], env: [String:Int]) -> Int {`, '    values := [Int]{}', '    loop node in nodes {', '        value :: if node == {', '            .Lit(v) -> v', '            .Var(name) -> symbol_lookup(env, name)',
      `            .Add(left, right) -> (values[left] + values[right]) % ${M}`, `            .Mul(left, right) -> (values[left] * values[right]) % ${M}`, '            .Neg(index) -> 0 - values[index]', '        }', '        &values.push(value)', '    }', '    values[values.len() - 1]', '}', '',
      `fn eval${i}_fold(x: Int) -> Int { (x * ${r.int(2, 9)}) % 1009 }`, '',
      `fn eval${i}_main(seed: Int) -> Int {`, '    env := [String:Int]{}', '    env["x"] = seed % 50', `    env["y"] = ${r.int(1, 99)}`,
      `    nodes :: [Node${i}]{Node${i}.Lit(${r.int(1, 9)}), Node${i}.Var("x"), Node${i}.Add{left: 0, right: 1}, Node${i}.Var("y"), Node${i}.Mul{left: 2, right: 3}, Node${i}.Neg(4), Node${i}.Add{left: 4, right: 5}}`,
      `    (eval${i}(nodes, env) + ${M} + ${helper('seed')}) % ${M}`, '}',
    ],
  }),
  (i, r, helper) => ({
    entry: `vm${i}_main`, helpers: [],
    lines: [
      `enum Op${i} {`, '    Push(Int)', '    Add', '    Sub', '    Mul', '    Dup', '    Swap', '    Drop', '    Jz(Int)', '    Jmp(Int)', '    Halt', '}', '',
      `fn vm${i}_run(program: [Op${i}]) -> Int {`, '    stack := [Int]{}', '    pc := 0', '    fuel := 64', '    loop pc < program.len() && fuel > 0 {', '        fuel -= 1', '        op :: program[pc]', '        pc += 1', '        if op == {',
        '            .Push(value) -> &stack.push(value)', '            .Add -> {', '                right :: &stack.pop() ?? 0', '                left :: &stack.pop() ?? 0', `                &stack.push((left + right) % ${M})`, '            }',
        '            .Sub -> {', '                right :: &stack.pop() ?? 0', '                left :: &stack.pop() ?? 0', '                &stack.push(left - right)', '            }',
        '            .Mul -> {', '                right :: &stack.pop() ?? 0', '                left :: &stack.pop() ?? 0', `                &stack.push((left * right) % ${M})`, '            }',
        '            .Dup -> {', '                top :: stack.last() ?? 0', '                &stack.push(top)', '            }', '            .Swap -> {', '                right :: &stack.pop() ?? 0', '                left :: &stack.pop() ?? 0', '                &stack.push(right)', '                &stack.push(left)', '            }',
        '            .Drop -> {', '                dropped :: &stack.pop() ?? 0', '                fuel -= dropped % 2', '            }', '            .Jz(target) -> {', '                top :: &stack.pop() ?? 0', '                if top == 0 -> pc = target', '            }', '            .Jmp(target) -> pc = target', '            .Halt -> pc = program.len()', '        }', '    }',
      '    stack.last() ?? 0', '}', '',
      `fn vm${i}_main(seed: Int) -> Int {`, `    program :: [Op${i}]{Op${i}.Push(seed % 10), Op${i}.Push(${r.int(2, 9)}), Op${i}.Mul, Op${i}.Dup, Op${i}.Push(3), Op${i}.Sub, Op${i}.Swap, Op${i}.Drop, Op${i}.Push(0), Op${i}.Jz(11), Op${i}.Jmp(12), Op${i}.Halt}`,
      `    (vm${i}_run(program) + ${M} + ${helper('seed')}) % ${M}`, '}',
    ],
  }),
  (i, r, helper) => ({
    entry: `types${i}_main`, helpers: [],
    lines: [
      `enum Ty${i} {`, '    IntT', '    BoolT', '    StrT', '    ListT(Int)', '    FnT(params: Int, result: Int)', '}', '',
      `fn ty${i}_code(ty: Ty${i}) -> Int {`, '    if ty == {', '        .IntT -> 1', '        .BoolT -> 2', '        .StrT -> 3', '        .ListT(element) -> 10 + element', '        .FnT(params, result) -> 100 + params * 10 + result', '    }', '}', '',
      `fn ty${i}_unify(left: Ty${i}, right: Ty${i}) -> Bool { ty${i}_code(left) == ty${i}_code(right) }`, '',
      `fn types${i}_main(seed: Int) -> Int {`, `    types :: [Ty${i}]{Ty${i}.IntT, Ty${i}.ListT(seed % 3), Ty${i}.FnT{params: 2, result: ${r.int(1, 3)}}, Ty${i}.StrT, Ty${i}.ListT(1)}`, '    matches := 0',
      '    loop left in types {', '        loop right in types {', `            if ty${i}_unify(left, right) -> matches += 1`, '        }', '    }', `    (matches + ${helper('matches')}) % ${M}`, '}',
    ],
  }),
];

// ----------------------------------------------------------- scientific

const scientificCommon = [
  'fn to_score(v: Float) -> Int {', '    if v < 0.0 || v > 1000000.0 -> return 0', '    (Int.from_float(v) ?? 0) % 1000003', '}', '',
  'fn absf(v: Float) -> Float { if v < 0.0 -> 0.0 - v else -> v }', '',
];

const scientific = [
  (i, r, helper) => ({
    entry: `poly${i}_main`, helpers: [],
    lines: [
      `fn poly${i}_coefficients() -> [Float] { [Float]{${Array.from({ length: 6 }, () => `${r.int(0, 9)}.${r.int(0, 9)}`).join(', ')}} }`, '',
      `fn poly${i}_eval(x: Float) -> Float {`, '    total := 0.0', `    loop c in poly${i}_coefficients() -> total = total * x + c`, '    total', '}', '',
      `fn poly${i}_newton(start: Float) -> Float {`, '    x := start', '    loop k in 0..<8 {', `        slope :: (poly${i}_eval(x + 0.001) - poly${i}_eval(x)) / 0.001`, '        if absf(slope) < 0.000001 -> break', `        x = x - poly${i}_eval(x) / slope + Float.from_int(k) * 0.0`, '    }', '    x', '}', '',
      `fn poly${i}_main(seed: Int) -> Int { (to_score(absf(poly${i}_newton(Float.from_int(seed % 10))) * 100.0) + ${helper('seed')}) % ${M} }`,
    ],
  }),
  (i, r, helper) => ({
    entry: `integrate${i}_main`, helpers: [`integrate${i}_steps`],
    lines: [
      `fn integrate${i}(f: fn(Float) -> Float, low: Float, high: Float, steps: Int) -> Float {`, '    width :: (high - low) / Float.from_int(steps)', '    total := 0.5 * (f(low) + f(high))',
      '    loop k in 1..<steps -> total = total + f(low + width * Float.from_int(k))', '    total * width', '}', '',
      `fn integrate${i}_steps(x: Int) -> Int { x % ${r.int(16, 64)} + 4 }`, '',
      `fn integrate${i}_main(seed: Int) -> Int {`, `    scale :: ${r.int(1, 5)}.25`, `    area :: integrate${i}((x) -> x * x * scale + 1.0, 0.0, Float.from_int(seed % 5 + 1), integrate${i}_steps(seed))`,
      `    (to_score(area) + ${helper('seed')}) % ${M}`, '}',
    ],
  }),
  (i, r, helper) => ({
    entry: `matrix${i}_main`, helpers: [],
    lines: [
      `fn matrix${i}_mul(a: [Float], b: [Float], n: Int) -> [Float] {`, '    out := [Float]{}', '    loop row in 0..<n {', '        loop col in 0..<n {', '            total := 0.0',
      '            loop k in 0..<n -> total = total + a[row * n + k] * b[k * n + col]', '            &out.push(total)', '        }', '    }', '    out', '}', '',
      `fn matrix${i}_trace(m: [Float], n: Int) -> Float {`, '    total := 0.0', '    loop k in 0..<n -> total = total + m[k * n + k]', '    total', '}', '',
      `fn matrix${i}_main(seed: Int) -> Int {`, '    a := [Float]{}', '    b := [Float]{}', '    loop k in 0..<16 {', `        &a.push(Float.from_int((seed + k * ${r.int(2, 9)}) % 10))`, '        &b.push(Float.from_int((k * 3) % 7) * 0.5)', '    }',
      `    (to_score(matrix${i}_trace(matrix${i}_mul(a, b, 4), 4)) + ${helper('seed')}) % ${M}`, '}',
    ],
  }),
  (i, r, helper) => ({
    entry: `ode${i}_main`, helpers: [],
    lines: [
      `struct Oscillator${i} {`, '    position: Float', '    velocity: Float', '    damping: Float', '    stiffness: Float', '}', '',
      `fn ode${i}_step(state: Oscillator${i}, dt: Float) -> Oscillator${i} {`, '    accel :: 0.0 - state.stiffness * state.position - state.damping * state.velocity', '    mid_velocity :: state.velocity + accel * dt * 0.5',
      `    Oscillator${i}{position: state.position + mid_velocity * dt, velocity: state.velocity + accel * dt, damping: state.damping, stiffness: state.stiffness}`, '}', '',
      `fn ode${i}_main(seed: Int) -> Int {`, `    state := Oscillator${i}{position: 1.0, velocity: 0.0, damping: 0.${r.int(1, 9)}, stiffness: ${r.int(1, 9)}.0}`,
      `    loop k in 0..<(seed % 20 + 10) -> state = ode${i}_step(state, 0.01)`, `    (to_score(absf(state.position) * 1000.0) + ${helper('seed')}) % ${M}`, '}',
    ],
  }),
];

// ---------------------------------------------------------- generic-lib

const genericCommon = [
  'struct Boxed<T> {', '    value: T', '    version: Int', '}', '',
  'struct Keyed<K, V> {', '    key: K', '    value: V', '}', '',
  'fn boxed<T>(value: T, version: Int) -> Boxed<T> { Boxed<T>{value: value, version: version} }', '',
  'fn unbox<T>(box: Boxed<T>) -> T { box.value }', '',
  'fn keyed<K, V>(key: K, value: V) -> Keyed<K, V> { Keyed<K, V>{key: key, value: value} }', '',
  'fn first_or<T>(items: [T], fallback: T) -> T {', '    if items.len() > 0 -> return items[0]', '    fallback', '}', '',
  'fn find_index<T>(items: [T], keep: fn(T) -> Bool) -> Int {', '    loop (index, item) in items {', '        if keep(item) -> return index', '    }', '    -1', '}', '',
  'fn fold_ints<T>(items: [T], start: Int, f: fn(Int, T) -> Int) -> Int {', '    total := start', '    loop item in items -> total = f(total, item)', '    total', '}', '',
];

const genericLib = [
  (i, r, helper) => ({
    entry: `model${i}_main`, helpers: [`model${i}_rank`],
    lines: [
      `struct Model${i} {`, '    id: Int', '    label: String', '    score: Int', '}', '',
      `fn model${i}_rank(x: Int) -> Int { (x * ${r.int(2, 13)}) % 211 }`, '',
      `fn model${i}_main(seed: Int) -> Int {`, `    models := [Model${i}]{}`,
      `    loop k in 0..<(seed % 6 + 3) -> &models.push(Model${i}{id: k, label: "${r.pick(WORDS)}{k}", score: model${i}_rank(seed + k)})`,
      '    labels :: models.map((model) -> model.label)', '    scores :: models.map((model) -> model.score)',
      `    best :: first_or(models.filter((model) -> model.score > ${r.int(20, 120)}), Model${i}{id: -1, label: "", score: 0})`,
      '    found :: find_index(scores, (score) -> score % 2 == 0)', '    total :: fold_ints(models, 0, (acc, model) -> acc + model.score)',
      `    box :: boxed(best, ${i % 97})`, '    pair :: keyed(best.label, total)',
      `    (labels.len() + found + total + unbox(box).id + box.version + pair.key.len() + pair.value + ${helper('seed')}) % ${M}`, '}',
    ],
  }),
  (i, r, helper) => ({
    entry: `cache${i}_main`, helpers: [],
    lines: [
      `fn cache${i}_lookup<T>(entries: [Keyed<String, T>], key: String, fallback: T) -> T {`, '    loop entry in entries {', '        if entry.key == key -> return entry.value', '    }', '    fallback', '}', '',
      `fn cache${i}_main(seed: Int) -> Int {`, `    ints :: [Keyed<String, Int>]{keyed("a", seed % 10), keyed("b", ${r.int(1, 99)})}`,
      `    texts :: [Keyed<String, String>]{keyed("x", "${r.pick(WORDS)}"), keyed("y", "z{seed % 3}")}`, '    floats :: [Keyed<String, Float>]{keyed("f", Float{1.5})}',
      `    hit :: cache${i}_lookup(ints, "b", 0)`, `    text :: cache${i}_lookup(texts, "y", "")`, `    ratio :: cache${i}_lookup(floats, "g", Float{2.5})`,
      `    (hit + text.len() + (Int.from_float(ratio) ?? 0) + ${helper('hit')}) % ${M}`, '}',
    ],
  }),
];

// ------------------------------------------------------------ generated

const FIELD_TYPES = [['Int', '0', v => `${v}`], ['String', '""', v => `"f${v}"`], ['Bool', 'false', v => (v % 2 ? 'true' : 'false')]];
const generatedKinds = [
  (i, r, helper) => {
    const fields = Array.from({ length: 16 }, (_, f) => [`field_${f}`, FIELD_TYPES[(f + i) % 3]]);
    return {
      entry: `message${i}_main`, helpers: [],
      lines: [
        `// Generated message ${i}; do not edit.`, `struct Message${i} {`, ...fields.map(([name, [type]]) => `    ${name}: ${type}`), '}', '',
        `fn message${i}_default() -> Message${i} { Message${i}{${fields.map(([name, [, zero]]) => `${name}: ${zero}`).join(', ')}} }`, '',
        `fn message${i}_sample(seed: Int) -> Message${i} { Message${i}{${fields.map(([name, [type, , value]], f) => `${name}: ${type === 'Int' ? `seed + ${f}` : value(f)}`).join(', ')}} }`, '',
        `fn message${i}_encode(message: Message${i}) -> String {`, '    out := ""', ...fields.map(([name], f) => `    out = "{out}${f + 1}:{message.${name}};"`), '    out', '}', '',
        `fn message${i}_field_name(number: Int) -> String {`, '    if number == {', ...fields.map(([name], f) => `        ${f + 1} -> "${name}"`), '        else -> "unknown"', '    }', '}', '',
        `fn message${i}_main(seed: Int) -> Int { (message${i}_encode(message${i}_sample(seed)).len() + message${i}_encode(message${i}_default()).len() + message${i}_field_name(seed % 18).len() + ${helper('seed')}) % ${M} }`,
      ],
    };
  },
  (i, r, helper) => {
    const variants = Array.from({ length: 24 }, (_, v) => `Kind${v}`);
    return {
      entry: `registry${i}_main`, helpers: [`registry${i}_code`],
      lines: [
        `// Generated registry ${i}; do not edit.`, `enum Registry${i} {`, ...variants.map(v => `    ${v}`), '}', '',
        `fn registry${i}_to_code(kind: Registry${i}) -> Int {`, '    if kind == {', ...variants.map((v, k) => `        .${v} -> ${k * 3 + 1}`), '    }', '}', '',
        `fn registry${i}_from_code(code: Int) -> Registry${i} {`, '    if code == {', ...variants.map((v, k) => `        ${k * 3 + 1} -> Registry${i}.${v}`), `        else -> Registry${i}.Kind0`, '    }', '}', '',
        `fn registry${i}_code(x: Int) -> Int { registry${i}_to_code(registry${i}_from_code(x % 72)) }`, '',
        `fn registry${i}_main(seed: Int) -> Int { (registry${i}_code(seed) + ${helper('seed')}) % ${M} }`,
      ],
    };
  },
  (i, r, helper) => {
    const table = Array.from({ length: 128 }, () => r.int(0, 9999));
    return {
      entry: `lut${i}_main`, helpers: [`lut${i}_at`],
      lines: [
        `// Generated lookup table ${i}; do not edit.`, `fn lut${i}() -> [Int] {`, '    [Int]{', ...Array.from({ length: 8 }, (_, row) => `        ${table.slice(row * 16, row * 16 + 16).join(', ')},`), '    }', '}', '',
        `fn lut${i}_at(x: Int) -> Int { lut${i}()[x % 128] }`, '',
        `fn lut${i}_main(seed: Int) -> Int { (lut${i}_at(seed) + lut${i}_at(seed + 64) + ${helper('seed')}) % ${M} }`,
      ],
    };
  },
  (i, r, helper) => {
    const fields = ['alpha', 'beta', 'gamma', 'delta', 'epsilon', 'zeta', 'eta', 'theta'];
    return {
      entry: `accessors${i}_main`, helpers: [],
      lines: [
        `// Generated accessors ${i}; do not edit.`, `struct Record${i} {`, ...fields.map(f => `    ${f}: Int`), '}', '',
        `impl Record${i} {`, ...fields.flatMap(f => [`    fn get_${f}(self) -> Int { self.${f} }`, `    fn set_${f}(&self, value: Int) { self.${f} = value }`]), '}', '',
        `fn accessors${i}_main(seed: Int) -> Int {`, `    record := Record${i}{${fields.map((f, k) => `${f}: ${k}`).join(', ')}}`, ...fields.map((f, k) => `    &record.set_${f}(seed + ${k})`),
        `    (${fields.map(f => `record.get_${f}()`).join(' + ')} + ${helper('seed')}) % ${M}`, '}',
      ],
    };
  },
];

export const DOMAINS = Object.freeze({
  web: { dimension: 'web/backend service: models, repositories, routes, templates, queries, roles', build: (n, r) => build('web', webCommon, web, n, r) },
  game: { dimension: 'game: components and systems, AI state machines, wave tables, inventories, behaviors', build: (n, r) => build('game', gameCommon, game, n, r) },
  cli: { dimension: 'CLI tool: argument parsing, commands, table output, configuration', build: (n, r) => build('cli', cliCommon, cli, n, r) },
  data: { dimension: 'data analysis: columns, group-by, pipelines, CSV rows', build: (n, r) => build('data', dataCommon, data, n, r) },
  gui: { dimension: 'GUI application: widgets and layout, reducers, styles, events', build: (n, r) => build('gui', guiCommon, gui, n, r) },
  embedded: { dimension: 'embedded: registers and bit fields, ring buffers, CRC tables, framing state machines', build: (n, r) => build('embedded', embeddedCommon, embedded, n, r) },
  compiler: { dimension: 'compiler/interpreter: lexers, ASTs and evaluators, stack VMs, type tables', build: (n, r) => build('compiler', compilerCommon, compiler, n, r) },
  scientific: { dimension: 'scientific/numeric: polynomials, quadrature, matrices, ODE steps', build: (n, r) => build('scientific', scientificCommon, scientific, n, r) },
  'generic-lib': { dimension: 'generic-heavy library in one entry file: generic records and functions instantiated per model', build: (n, r) => build('generic_lib', genericCommon, genericLib, n, r, { entryFile: true }) },
  generated: { dimension: 'generated code: message structs, registries, lookup tables, accessors', build: (n, r) => build('generated', [], generatedKinds, n, r) },
});
