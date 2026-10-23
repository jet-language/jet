// Type-directed fuzz case generator for the post-Rust safety net (#4619
// criterion 3).
//
// Each case is a generated Jet program and the stdout a model evaluator
// computes for it. prepareCorpus in safety-gate.mjs writes them in the
// Examples/features layout:
//   <out>/corpus/Examples/features/fuzz/s<seed>_<n>.jet
//   <out>/corpus/Examples/features/expected/fuzz/s<seed>_<n>.out
// and the standing native safety gate checks admitted native outputs of them.
//
// The generator draws from the syntax the feature goldens and the memory
// corpus use: Int and Bool bindings (`::`, `:=` only when the program mutates
// them), every Int operator at Jet's precedence with minimal parentheses,
// compound assignment, `if` statements, arm tables and `if` values, counted,
// conditional and list loops with `break` and `next`, lists (literal, push,
// indexed read and write, len, printing), one struct (literal, field write,
// printing), Strings (literals, interpolation, `+=`, `.copy()` copies, moves out of
// a binding, which then stays unused, `len`, equality), helper functions
// taking Int, String, list and struct parameters, early returns, a bounded
// recursive helper, and prints inside helpers (so call and short-circuit
// order are observable). Every choice is typed by the bindings in
// scope. The model evaluates the program with i64 semantics (`/%` and `%`
// floor, `%%` truncates, `>>` is arithmetic); a program whose evaluation
// overflows, divides by zero, indexes out of range or runs too long is
// discarded and redrawn, so every expected output is defined behavior.
// Indexes are `e % xs.len()` (floored, so in range) or literal 0, and divisors
// are mostly `e % k + 1`, which keeps the discard rate low. A failing case
// reproduces from its seed; the .jet is the repro.
import { createHash } from "node:crypto";

const I64_MAX = (1n << 63n) - 1n;
const I64_MIN = -(1n << 63n);
const STEP_LIMIT = 20000;
const OUTPUT_LIMIT = 300;
// `s += s` in a loop doubles a String; the model discards such programs long
// before the evaluator's own string limit.
const STRING_LIMIT = 4096;
const ATTEMPTS = 200;

class Reject extends Error {}

// mulberry32 over a per-attempt seed derived from (seed, case, attempt).
function random(...key) {
  let state = createHash("sha256").update(key.join(":")).digest().readUInt32LE(0);
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const int = (v) => ({ k: "int", v: BigInt(v) });
const bin = (op, l, r) => ({ k: "bin", op, l, r });
const COMPARE = ["==", "!=", "<", ">", "<=", ">="];
const SIMPLE = new Set(["assign", "print", "push", "setidx", "setfield", "break", "next", "return"]);

// ---------------------------------------------------------------- generator

class Gen {
  constructor(rand) {
    this.r = rand;
    this.n = 0;
    this.scopes = [];
    this.fns = [];
    this.struct = null;
    this.current = null; // the function being generated
  }
  int(n) { return Math.floor(this.r() * n); }
  chance(p) { return this.r() < p; }
  pick(items) { return items[this.int(items.length)]; }
  weighted(pairs) {
    const live = pairs.filter(([, w]) => w > 0);
    let x = this.r() * live.reduce((sum, [, w]) => sum + w, 0);
    for (const [value, w] of live) if ((x -= w) < 0) return value;
    return live.at(-1)[0];
  }
  fresh(prefix) { return `${prefix}${this.n++}`; }
  // A moved String binding is gone for the rest of the program.
  vars(pred) { return this.scopes.flat().filter((v) => !v.moved && pred(v)); }
  declare(v) { this.scopes.at(-1).push(v); return v; }

  // Expressions.
  literal() {
    const kind = this.weighted([["small", 6], ["negative", 2], ["medium", 1], ["large", 1], ["extreme", 0.3]]);
    if (kind === "small") return int(this.int(21));
    if (kind === "negative") return int(-1 - this.int(20));
    if (kind === "medium") return int(this.int(1000));
    if (kind === "large") return int(BigInt(this.pick([-1, 1])) * ((1n << BigInt(20 + this.int(42))) + BigInt(this.int(1000))));
    return int(this.pick([I64_MAX, -I64_MAX]));
  }
  intLeaf(allowIndex = true) {
    const ints = this.vars((v) => v.type === "Int");
    const lists = this.vars((v) => v.type === "List");
    const structs = this.vars((v) => v.type === "Struct");
    const strs = this.vars((v) => v.type === "Str");
    const kind = this.weighted([
      ["lit", 4], ["var", ints.length ? 6 : 0], ["len", lists.length ? 1 : 0],
      ["index", lists.length && allowIndex ? 2 : 0], ["field", structs.length ? 2 : 0], ["slen", strs.length ? 1 : 0],
    ]);
    if (kind === "var") return { k: "var", name: this.pick(ints).name };
    if (kind === "len") return { k: "len", list: this.pick(lists).name };
    if (kind === "slen") return { k: "len", list: this.pick(strs).name };
    if (kind === "field") return { k: "field", base: this.pick(structs).name, f: this.pick(this.struct.fields) };
    if (kind === "index") {
      const list = this.pick(lists).name;
      const idx = this.chance(0.25) ? int(0) : bin("%", this.intLeaf(false), { k: "len", list });
      return { k: "index", list, idx };
    }
    return this.literal();
  }
  divisor(depth) { return bin("+", bin("%", this.intExpr(depth), int(2 + this.int(8))), int(1)); }
  callable() {
    return this.fns.filter((fn) => fn !== this.current && fn.params.every((p) => p.type === "Int" || this.vars((v) => v.type === p.type).length));
  }
  call(fn, depth) {
    const args = fn.params.map((p, i) => {
      if (p.type !== "Int") return { k: "var", name: this.pick(this.vars((v) => v.type === p.type)).name };
      const arg = this.intExpr(depth);
      return fn.guard?.[i] ? bin("%", arg, int(fn.guard[i])) : arg;
    });
    return { k: "call", fn: fn.name, args };
  }
  intExpr(depth) {
    if (depth <= 0 || this.chance(0.25)) return this.intLeaf();
    const callable = this.callable();
    const kind = this.weighted([["arith", 8], ["div", 2], ["bit", 2], ["shift", 1], ["neg", 1], ["call", callable.length ? 2 : 0]]);
    const d = depth - 1;
    if (kind === "arith") return bin(this.weighted([["+", 3], ["-", 3], ["*", 2]]), this.intExpr(d), this.intExpr(d));
    if (kind === "div") return bin(this.pick(["/%", "%", "%%"]), this.intExpr(d), this.chance(0.8) ? this.divisor(d) : this.intExpr(d));
    if (kind === "bit") return bin(this.pick(["&", "|", "~|"]), this.intExpr(d), this.intExpr(d));
    if (kind === "shift") return bin(this.pick(["<<", ">>"]), this.intExpr(d), int(this.int(9)));
    if (kind === "neg") return { k: "neg", e: this.intExpr(d) };
    return this.call(this.pick(callable), d);
  }
  boolExpr(depth) {
    const bools = this.vars((v) => v.type === "Bool");
    const strs = this.vars((v) => v.type === "Str");
    if (depth <= 0 || this.chance(0.2)) {
      const kind = this.weighted([["lit", 1], ["var", bools.length ? 3 : 0], ["cmp", 4], ["scmp", strs.length ? 1 : 0]]);
      if (kind === "lit") return { k: "bool", v: this.chance(0.5) };
      if (kind === "var") return { k: "var", name: this.pick(bools).name };
      if (kind === "scmp") return bin(this.pick(["==", "!="]), { k: "var", name: this.pick(strs).name }, this.chance(0.5) ? { k: "var", name: this.pick(strs).name } : this.text());
      return bin(this.pick(COMPARE), this.intExpr(1), this.intExpr(1));
    }
    const kind = this.weighted([["cmp", 6], ["and", 2], ["or", 2], ["not", 1]]);
    const d = depth - 1;
    if (kind === "cmp") return bin(this.pick(COMPARE), this.intExpr(d), this.intExpr(d));
    if (kind === "not") return { k: "not", e: this.boolExpr(d) };
    return bin(kind === "and" ? "&&" : "||", this.boolExpr(d), this.boolExpr(d));
  }
  ifValue() {
    const arms = Array.from({ length: 1 + this.int(3) }, () => [this.boolExpr(2), this.intExpr(2)]);
    return { k: "ifx", arms, otherwise: this.intExpr(2) };
  }
  // String values: literals, interpolations of the bindings in scope, and
  // explicit copies (`s.copy()`). Moves (`t :: s`) are generated as bindings.
  text() {
    const letters = "abcdefghij klm-:.,";
    return { k: "str", v: Array.from({ length: this.int(6) }, () => letters[this.int(letters.length)]).join("") };
  }
  strExpr() {
    const strs = this.vars((v) => v.type === "Str");
    const holes = [
      ...this.vars((v) => v.type === "Int").map((v) => ({ k: "var", name: v.name })),
      ...strs.map((v) => ({ k: "var", name: v.name })),
      ...strs.map((v) => ({ k: "len", list: v.name })),
    ];
    const kind = this.weighted([["lit", 3], ["fmt", holes.length ? 3 : 0], ["copy", strs.length ? 2 : 0]]);
    if (kind === "copy") return { k: "copy", name: this.pick(strs).name };
    if (kind === "lit") return this.text();
    const parts = [];
    for (let i = 0, n = 1 + this.int(3); i < n; i++) parts.push(this.text().v, this.pick(holes));
    if (this.chance(0.5)) parts.push(this.text().v);
    return { k: "strfmt", parts };
  }

  // Statements. ctx: { loops (nesting), inLoop, inFn, prints, nest }.
  block(count, ctx) {
    this.scopes.push([]);
    const out = [];
    for (let i = 0; i < count; i++) out.push(...this.stmt(ctx));
    this.scopes.pop();
    return out;
  }
  stmt(ctx) {
    const ints = this.vars((v) => v.type === "Int" && v.mutable);
    const bools = this.vars((v) => v.type === "Bool" && v.mutable);
    const lists = this.vars((v) => v.type === "List");
    const open = lists.filter((v) => v.mutable && !v.frozen);
    const structs = this.vars((v) => v.type === "Struct" && v.mutable);
    const strs = this.vars((v) => v.type === "Str" && v.mutable);
    // A move out of a binding made in an enclosing loop would repeat (E: use after move).
    const movable = strs.filter((v) => v.loops === ctx.loops);
    const deeper = ctx.nest < 3;
    const kind = this.weighted([
      ["int", 5], ["bool", 1], ["assign", ints.length ? 4 : 0], ["assignBool", bools.length ? 0.5 : 0],
      ["print", ctx.prints ? 4 : 0], ["if", deeper ? 3 : 0],
      ["range", ctx.loops < 2 && deeper ? 2 : 0], ["while", ctx.loops < 2 && deeper ? 1.5 : 0],
      ["list", 1.5], ["push", open.length ? 1.5 : 0], ["setidx", open.length ? 1 : 0],
      ["foreach", lists.length && ctx.loops < 2 && deeper ? 1.5 : 0],
      ["struct", this.struct ? 1 : 0], ["setfield", structs.length ? 1.5 : 0],
      ["str", 2], ["append", strs.length ? 2 : 0], ["assignStr", strs.length ? 0.5 : 0], ["move", movable.length ? 0.7 : 0],
      ["jump", ctx.inLoop ? 1 : 0], ["return", ctx.inFn ? 0.7 : 0],
    ]);
    const inner = { ...ctx, nest: ctx.nest + 1 };
    const body = (count, extra = {}) => this.block(count, { ...inner, ...extra });
    switch (kind) {
      case "int": {
        const init = this.chance(0.1) ? this.ifValue() : this.intExpr(2);
        // `v :: xs[i]` and `v :: p.f` bind a read view that blocks later writes
        // to the owner (E0212); `:=` binds an owned copy.
        const copy = init.k === "index" || init.k === "field";
        return [{ k: "decl", v: this.declare({ name: this.fresh("v"), type: "Int", mutable: true, mutated: copy }), init }];
      }
      case "bool": {
        const init = this.boolExpr(2);
        return [{ k: "decl", v: this.declare({ name: this.fresh("b"), type: "Bool", mutable: true, mutated: false }), init }];
      }
      case "assign": {
        const v = this.pick(ints);
        v.mutated = true;
        const op = this.weighted([["=", 3], ["+=", 3], ["-=", 2], ["*=", 1], ["/%=", 1], ["%=", 1]]);
        return [{ k: "assign", name: v.name, op, e: op === "/%=" || op === "%=" ? this.divisor(1) : this.intExpr(2) }];
      }
      case "assignBool": {
        const v = this.pick(bools);
        v.mutated = true;
        return [{ k: "assign", name: v.name, op: "=", e: this.boolExpr(2) }];
      }
      case "str": {
        const init = this.strExpr();
        return [{ k: "decl", v: this.declare({ name: this.fresh("s"), type: "Str", mutable: true, mutated: false, loops: ctx.loops }), init }];
      }
      case "append": {
        const v = this.pick(strs);
        v.mutated = true;
        const all = this.vars((x) => x.type === "Str");
        return [{ k: "assign", name: v.name, op: "+=", e: this.chance(0.3) ? { k: "var", name: this.pick(all).name } : this.strExpr() }];
      }
      case "assignStr": {
        const v = this.pick(strs);
        v.mutated = true;
        return [{ k: "assign", name: v.name, op: "=", e: this.strExpr() }];
      }
      case "move": {
        const source = this.pick(movable);
        const init = { k: "var", name: source.name };
        source.moved = true;
        return [{ k: "decl", v: this.declare({ name: this.fresh("s"), type: "Str", mutable: true, mutated: false, loops: ctx.loops }), init }];
      }
      case "print": return [this.print()];
      case "if": {
        const arms = Array.from({ length: 1 + this.int(3) }, () => [this.boolExpr(2), body(1 + this.int(3))]);
        return [{ k: "if", arms, otherwise: this.chance(0.4) ? body(1 + this.int(2)) : null }];
      }
      case "range": {
        const lo = this.chance(0.7) ? int(0) : int(this.int(4));
        const hi = this.chance(0.6) ? int(this.int(6)) : bin("%", this.intExpr(1), int(2 + this.int(5)));
        const v = { name: this.fresh("i"), type: "Int", mutable: false };
        this.scopes.push([v]);
        const stmts = body(1 + this.int(4), { loops: ctx.loops + 1, inLoop: true });
        this.scopes.pop();
        return [{ k: "range", v: v.name, lo, hi, inclusive: this.chance(0.3), body: stmts }];
      }
      case "while": {
        const counter = this.declare({ name: this.fresh("c"), type: "Int", mutable: false, mutated: true });
        const stmts = body(1 + this.int(4), { loops: ctx.loops + 1, inLoop: true });
        return [
          { k: "decl", v: counter, init: int(0) },
          { k: "while", counter: counter.name, limit: 1 + this.int(6), body: stmts },
        ];
      }
      case "list": {
        const elems = Array.from({ length: 1 + this.int(4) }, () => this.intExpr(1));
        return [{ k: "decl", v: this.declare({ name: this.fresh("xs"), type: "List", mutable: true, mutated: false, frozen: 0 }), init: { k: "list", elems } }];
      }
      case "push": {
        const v = this.pick(open);
        v.mutated = true;
        return [{ k: "push", list: v.name, e: this.intExpr(2) }];
      }
      case "setidx": {
        const v = this.pick(open);
        v.mutated = true;
        const idx = this.chance(0.25) ? int(0) : bin("%", this.intLeaf(false), { k: "len", list: v.name });
        // Compound assignment cannot target an indexed slot (E0164).
        return [{ k: "setidx", list: v.name, idx, op: "=", e: this.intExpr(2) }];
      }
      case "foreach": {
        const list = this.pick(lists);
        const index = this.chance(0.4) ? { name: this.fresh("i"), type: "Int", mutable: false } : null;
        const item = { name: this.fresh("x"), type: "Int", mutable: false };
        list.frozen++;
        this.scopes.push(index ? [index, item] : [item]);
        const stmts = body(1 + this.int(3), { loops: ctx.loops + 1, inLoop: true });
        this.scopes.pop();
        list.frozen--;
        return [{ k: "foreach", list: list.name, index: index?.name ?? null, item: item.name, body: stmts }];
      }
      case "struct": {
        const fields = this.struct.fields.map((f) => [f, this.intExpr(1)]);
        return [{ k: "decl", v: this.declare({ name: this.fresh("p"), type: "Struct", mutable: true, mutated: false }), init: { k: "struct", sname: this.struct.name, fields } }];
      }
      case "setfield": {
        const v = this.pick(structs);
        v.mutated = true;
        return [{ k: "setfield", base: v.name, f: this.pick(this.struct.fields), op: this.pick(["=", "+=", "-="]), e: this.intExpr(2) }];
      }
      case "jump": return [{ k: "if", arms: [[this.boolExpr(1), [{ k: this.chance(0.5) ? "break" : "next" }]]], otherwise: null }];
      default: return [{ k: "if", arms: [[this.boolExpr(1), [{ k: "return", e: this.intExpr(2) }]]], otherwise: null }];
    }
  }
  print() {
    const ints = this.vars((v) => v.type === "Int");
    const bools = this.vars((v) => v.type === "Bool");
    const lists = this.vars((v) => v.type === "List");
    const structs = this.vars((v) => v.type === "Struct");
    const strs = this.vars((v) => v.type === "Str");
    const kind = this.weighted([["interp", 4], ["expr", 2], ["bool", 1], ["list", lists.length ? 1 : 0], ["struct", structs.length ? 0.7 : 0], ["str", strs.length ? 1 : 0]]);
    if (kind === "str") return { k: "print", e: { k: "var", name: this.pick(strs).name } };
    if (kind === "expr") return { k: "print", e: this.intExpr(2) };
    if (kind === "bool") return { k: "print", e: this.boolExpr(2) };
    if (kind === "list") return { k: "print", e: { k: "var", name: this.pick(lists).name } };
    if (kind === "struct") return { k: "print", e: { k: "var", name: this.pick(structs).name } };
    const holes = [
      ...ints.map((v) => ({ k: "var", name: v.name })),
      ...bools.map((v) => ({ k: "var", name: v.name })),
      ...strs.map((v) => ({ k: "var", name: v.name })),
      ...structs.flatMap((v) => this.struct.fields.map((f) => ({ k: "field", base: v.name, f }))),
    ];
    const parts = [this.fresh("t")];
    for (let i = 0, n = holes.length ? 1 + this.int(3) : 0; i < n; i++) parts.push(i ? " " : " = ", this.pick(holes));
    return { k: "interp", parts };
  }

  helper(index) {
    const types = ["Int", "Int", "Int", "Int", "List", "Str", ...(this.struct ? ["Struct"] : [])];
    const params = Array.from({ length: 1 + this.int(3) }, (_, i) => ({ name: `a${index}_${i}`, type: this.pick(types), mutable: false }));
    const fn = { name: `h${index}`, params, body: [] };
    this.current = fn;
    this.scopes = [params];
    const ctx = { loops: 0, inLoop: false, inFn: true, prints: this.chance(0.3), nest: 1 };
    fn.body = [...this.block(1 + this.int(5), ctx), { k: "return", e: this.intExpr(2) }];
    this.current = null;
    return fn;
  }
  recursive(index) {
    const n = { name: "n", type: "Int", mutable: false };
    const acc = { name: "acc", type: "Int", mutable: false };
    const fn = { name: `r${index}`, params: [n, acc], guard: [12, null], body: [] };
    this.current = fn;
    this.scopes = [[n, acc]];
    const ctx = { loops: 0, inLoop: false, inFn: true, prints: false, nest: 2 };
    const base = { k: "if", arms: [[bin("<=", { k: "var", name: "n" }, int(0)), [{ k: "return", e: { k: "var", name: "acc" } }]]], otherwise: null };
    const mid = this.block(this.int(3), ctx);
    const step = { k: "call", fn: fn.name, args: [bin("-", { k: "var", name: "n" }, int(1)), this.intExpr(2)] };
    const tail = this.chance(0.5) ? step : bin(this.pick(["+", "-", "~|"]), step, this.intExpr(1));
    fn.body = [base, ...mid, { k: "return", e: tail }];
    this.current = null;
    return fn;
  }
  program() {
    if (this.chance(0.6)) {
      const fields = ["x", "y", "z", "w"].slice(0, 2 + this.int(3));
      this.struct = { name: `Pt${this.int(10)}`, fields };
    }
    const helpers = this.int(4);
    for (let i = 0; i < helpers; i++) this.fns.push(this.helper(i));
    if (this.chance(0.4)) this.fns.push(this.recursive(helpers));
    // `run` ends by printing every binding still in scope, so a computation
    // no other print observed still reaches stdout.
    this.scopes = [[]];
    const ctx = { loops: 0, inLoop: false, inFn: false, prints: true, nest: 0 };
    const run = Array.from({ length: 10 + this.int(20) }, () => this.stmt(ctx)).flat();
    for (const v of this.scopes[0].filter((v) => !v.moved)) run.push({ k: "print", e: { k: "var", name: v.name } });
    return { struct: this.struct, fns: this.fns, run };
  }
}

// ---------------------------------------------------------------- renderer

const PREC = { "||": 1, "&&": 2, "==": 3, "!=": 3, "<": 3, ">": 3, "<=": 3, ">=": 3, "|": 4, "~|": 5, "&": 6, "<<": 7, ">>": 7, "+": 8, "-": 8, "*": 9, "/%": 9, "%": 9, "%%": 9 };
const BIT_METHODS = { "&": "bit_and", "|": "bit_or", "~|": "bit_xor" };
const ATOM = 11;

function render(e) {
  const atom = (text) => ({ text, prec: ATOM });
  switch (e.k) {
    case "int": return atom(e.v < 0n ? `(${e.v})` : `${e.v}`);
    case "bool": return atom(String(e.v));
    case "str": return atom(`"${e.v}"`);
    case "strfmt": return atom(`"${e.parts.map((part) => (typeof part === "string" ? part : `{${expr(part)}}`)).join("")}"`);
    case "copy": return atom(`${e.name}.copy()`);
    case "var": return atom(e.name);
    case "len": return atom(`${e.list}.len()`);
    case "index": return atom(`${e.list}[${expr(e.idx)}]`);
    case "field": return atom(`${e.base}.${e.f}`);
    case "call": return atom(`${e.fn}(${e.args.map(expr).join(", ")})`);
    case "neg": return atom(`(-${grouped(e.e)})`);
    case "not": return { text: `!${grouped(e.e)}`, prec: 10 };
    case "bin": {
      const method = BIT_METHODS[e.op];
      if (method) return atom(`${grouped(e.l)}.${method}(${expr(e.r)})`);
      const p = PREC[e.op];
      const l = render(e.l);
      const r = render(e.r);
      // Comparisons do not chain, and `&&` mixed with `||` needs parentheses (E0082).
      const logic = (child) => p <= 2 && child.prec <= 2 && child.prec !== p;
      const left = l.prec < p || (p === 3 && l.prec === 3) || logic(l) ? `(${l.text})` : l.text;
      const right = r.prec <= p || logic(r) ? `(${r.text})` : r.text;
      return { text: `${left} ${e.op} ${right}`, prec: p };
    }
    default: throw new Error(`render: ${e.k}`);
  }
}
const expr = (e) => render(e).text;
function grouped(e) {
  const r = render(e);
  return r.prec === ATOM ? r.text : `(${r.text})`;
}

function simple(s) {
  switch (s.k) {
    case "assign": return `${s.name} ${s.op} ${expr(s.e)}`;
    case "print": return `print(${expr(s.e)})`;
    case "interp": return `print("${s.parts.map((part) => (typeof part === "string" ? part : `{${expr(part)}}`)).join("")}")`;
    case "push": return `&${s.list}.push(${expr(s.e)})`;
    case "setidx": return `${s.list}[${expr(s.idx)}] ${s.op} ${expr(s.e)}`;
    case "setfield": return `${s.base}.${s.f} ${s.op} ${expr(s.e)}`;
    case "break": return "break";
    case "next": return "next";
    case "return": return `return ${expr(s.e)}`;
    default: return null;
  }
}

function lines(stmts, depth) {
  const pad = "    ".repeat(depth);
  const out = [];
  const nested = (head, body) => {
    const one = body.length === 1 ? simple(body[0]) : null;
    if (one !== null) out.push(`${pad}${head} -> ${one}`);
    else out.push(`${pad}${head} {`, ...lines(body, depth + 1), `${pad}}`);
  };
  for (const s of stmts) {
    const one = simple(s);
    if (one !== null) {
      out.push(pad + one);
      continue;
    }
    switch (s.k) {
      case "decl": {
        const bind = `${pad}${s.v.name} ${s.v.mutated ? ":=" : "::"} `;
        const init = s.init;
        if (init.k === "list") out.push(`${bind}[${init.elems.map(expr).join(", ")}]`);
        else if (init.k === "struct") out.push(`${bind}${init.sname}{${init.fields.map(([f, e]) => `${f}: ${expr(e)}`).join(", ")}}`);
        else if (init.k === "ifx" && init.arms.length === 1) out.push(`${bind}if ${expr(init.arms[0][0])} -> ${expr(init.arms[0][1])} else -> ${expr(init.otherwise)}`);
        else if (init.k === "ifx") {
          out.push(`${bind}if {`);
          for (const [cond, value] of init.arms) out.push(`${pad}    ${expr(cond)} -> ${expr(value)}`);
          out.push(`${pad}    else -> ${expr(init.otherwise)}`, `${pad}}`);
        } else out.push(bind + expr(init));
        break;
      }
      case "if": {
        const one = s.arms.length === 1 && !s.otherwise && s.arms[0][1].length === 1 ? simple(s.arms[0][1][0]) : null;
        if (one !== null) {
          out.push(`${pad}if ${expr(s.arms[0][0])} -> ${one}`);
          break;
        }
        out.push(`${pad}if {`);
        for (const [cond, body] of s.arms) out.push(`${pad}    ${expr(cond)} -> {`, ...lines(body, depth + 2), `${pad}    }`);
        if (s.otherwise) out.push(`${pad}    else -> {`, ...lines(s.otherwise, depth + 2), `${pad}    }`);
        out.push(`${pad}}`);
        break;
      }
      case "range": nested(`loop ${s.v} in ${grouped(s.lo)}${s.inclusive ? ".." : "..<"}${grouped(s.hi)}`, s.body); break;
      case "while": out.push(`${pad}loop ${s.counter} < ${s.limit} {`, `${pad}    ${s.counter} += 1`, ...lines(s.body, depth + 1), `${pad}}`); break;
      case "foreach": nested(`loop ${s.index ? `(${s.index}, ${s.item})` : s.item} in ${s.list}`, s.body); break;
      default: throw new Error(`lines: ${s.k}`);
    }
  }
  return out;
}

const TYPE_NAMES = { Int: "Int", List: "[Int]", Str: "String" };
function source(program, header) {
  const out = [...header.map((line) => `// ${line}`)];
  if (program.struct) out.push("", `struct ${program.struct.name} {`, ...program.struct.fields.map((f) => `    ${f}: Int`), "}");
  for (const fn of program.fns) {
    const params = fn.params.map((p) => `${p.name}: ${TYPE_NAMES[p.type] ?? program.struct.name}`).join(", ");
    out.push("", `fn ${fn.name}(${params}) -> Int {`, ...lines(fn.body, 1), "}");
  }
  out.push("", "fn run() {", ...lines(program.run, 1), "}");
  return out.join("\n") + "\n";
}

// ---------------------------------------------------------------- model

class Env {
  constructor(parent) { this.parent = parent; this.vars = new Map(); }
  define(name, value) { this.vars.set(name, value); }
  owner(name) {
    for (let env = this; env; env = env.parent) if (env.vars.has(name)) return env;
    throw new Error(`model: unbound ${name}`);
  }
  get(name) { return this.owner(name).vars.get(name); }
  set(name, value) { this.owner(name).vars.set(name, value); }
}

function checked(v) {
  if (typeof v === "bigint" && (v > I64_MAX || v < I64_MIN)) throw new Reject("overflow");
  if (typeof v === "string" && v.length > STRING_LIMIT) throw new Reject("string size");
  return v;
}
function floorDiv(a, b) {
  if (b === 0n) throw new Reject("division by zero");
  const q = a / b;
  return a % b !== 0n && (a < 0n) !== (b < 0n) ? q - 1n : q;
}
function arith(op, a, b) {
  switch (op) {
    case "+": return checked(a + b);
    case "-": return checked(a - b);
    case "*": return checked(a * b);
    case "/%": return checked(floorDiv(a, b));
    case "%":
    case "%%":
      // i64::MIN by -1 overflows in the remainder too.
      if (b === 0n) throw new Reject("division by zero");
      if (a === I64_MIN && b === -1n) throw new Reject("overflow");
      return op === "%" ? a - b * floorDiv(a, b) : a % b;
    case "&": return a & b;
    case "|": return a | b;
    case "~|": return a ^ b;
    case "<<": return checked(a << b);
    case ">>": return a >> b;
    case "==": return a === b;
    case "!=": return a !== b;
    case "<": return a < b;
    case ">": return a > b;
    case "<=": return a <= b;
    case ">=": return a >= b;
    default: throw new Error(`model: ${op}`);
  }
}

class Machine {
  constructor(program) {
    this.fns = new Map(program.fns.map((fn) => [fn.name, fn]));
    this.out = [];
    this.steps = 0;
    this.depth = 0;
  }
  tick() { if (++this.steps > STEP_LIMIT) throw new Reject("steps"); }
  show(v) {
    if (typeof v === "bigint") return v.toString();
    if (typeof v === "boolean" || typeof v === "string") return String(v);
    if (v.items) return `[${v.items.join(", ")}]`;
    return `${v.sname} { ${Object.entries(v.f).map(([f, x]) => `${f}: ${x}`).join(", ")} }`;
  }
  emit(line) {
    this.out.push(line);
    if (this.out.length > OUTPUT_LIMIT) throw new Reject("output");
  }
  eval(e, env) {
    switch (e.k) {
      case "int": case "bool": case "str": return e.v;
      case "copy": return env.get(e.name);
      case "strfmt": return checked(e.parts.map((part) => (typeof part === "string" ? part : this.show(this.eval(part, env)))).join(""));
      case "var": return env.get(e.name);
      case "neg": return checked(-this.eval(e.e, env));
      case "not": return !this.eval(e.e, env);
      case "len": {
        const value = env.get(e.list);
        return BigInt(typeof value === "string" ? value.length : value.items.length);
      }
      case "field": return env.get(e.base).f[e.f];
      case "index": {
        const list = env.get(e.list);
        const i = this.eval(e.idx, env);
        if (i < 0n || i >= BigInt(list.items.length)) throw new Reject("index");
        return list.items[Number(i)];
      }
      case "call": return this.call(e, env);
      case "ifx": {
        for (const [cond, value] of e.arms) if (this.eval(cond, env)) return this.eval(value, env);
        return this.eval(e.otherwise, env);
      }
      case "list": return { items: e.elems.map((x) => this.eval(x, env)) };
      case "struct": return { sname: e.sname, f: Object.fromEntries(e.fields.map(([f, x]) => [f, this.eval(x, env)])) };
      case "bin": {
        if (e.op === "&&") return this.eval(e.l, env) && this.eval(e.r, env);
        if (e.op === "||") return this.eval(e.l, env) || this.eval(e.r, env);
        const a = this.eval(e.l, env);
        return arith(e.op, a, this.eval(e.r, env));
      }
      default: throw new Error(`model: ${e.k}`);
    }
  }
  call(e, env) {
    const fn = this.fns.get(e.fn);
    const args = e.args.map((arg) => this.eval(arg, env));
    if (++this.depth > 64) throw new Reject("depth");
    const frame = new Env(null);
    fn.params.forEach((p, i) => frame.define(p.name, args[i]));
    const signal = this.exec(fn.body, frame);
    this.depth--;
    if (signal?.k !== "return") throw new Error(`model: ${fn.name} fell off its end`);
    return signal.v;
  }
  update(op, current, value) { return op === "=" ? value : arith(op.slice(0, -1), current, value); }
  exec(stmts, env) {
    for (const s of stmts) {
      const signal = this.stmt(s, env);
      if (signal) return signal;
    }
    return null;
  }
  loopBody(stmts, env) {
    const signal = this.exec(stmts, env);
    if (signal?.k === "next") return null;
    return signal;
  }
  stmt(s, env) {
    this.tick();
    switch (s.k) {
      case "decl": env.define(s.v.name, this.eval(s.init, env)); return null;
      case "assign": {
        const value = this.eval(s.e, env);
        env.set(s.name, this.update(s.op, env.get(s.name), value));
        return null;
      }
      case "print": this.emit(this.show(this.eval(s.e, env))); return null;
      case "interp": this.emit(s.parts.map((part) => (typeof part === "string" ? part : this.show(this.eval(part, env)))).join("")); return null;
      case "push": {
        const value = this.eval(s.e, env);
        const list = env.get(s.list);
        list.items.push(value);
        if (list.items.length > 1000) throw new Reject("list size");
        return null;
      }
      case "setidx": {
        const list = env.get(s.list);
        const i = this.eval(s.idx, env);
        const value = this.eval(s.e, env);
        if (i < 0n || i >= BigInt(list.items.length)) throw new Reject("index");
        list.items[Number(i)] = this.update(s.op, list.items[Number(i)], value);
        return null;
      }
      case "setfield": {
        const value = this.eval(s.e, env);
        const target = env.get(s.base);
        target.f[s.f] = this.update(s.op, target.f[s.f], value);
        return null;
      }
      case "break": case "next": return { k: s.k };
      case "return": return { k: "return", v: this.eval(s.e, env) };
      case "if": {
        for (const [cond, body] of s.arms) if (this.eval(cond, env)) return this.exec(body, new Env(env));
        return s.otherwise ? this.exec(s.otherwise, new Env(env)) : null;
      }
      case "range": {
        const lo = this.eval(s.lo, env);
        const hi = this.eval(s.hi, env) + (s.inclusive ? 1n : 0n);
        for (let i = lo; i < hi; i++) {
          this.tick();
          const frame = new Env(env);
          frame.define(s.v, i);
          const signal = this.loopBody(s.body, frame);
          if (signal?.k === "break") break;
          if (signal) return signal;
        }
        return null;
      }
      case "while": {
        while (env.get(s.counter) < BigInt(s.limit)) {
          this.tick();
          env.set(s.counter, env.get(s.counter) + 1n);
          const signal = this.loopBody(s.body, new Env(env));
          if (signal?.k === "break") break;
          if (signal) return signal;
        }
        return null;
      }
      case "foreach": {
        const list = env.get(s.list);
        for (let i = 0; i < list.items.length; i++) {
          this.tick();
          const frame = new Env(env);
          if (s.index) frame.define(s.index, BigInt(i));
          frame.define(s.item, list.items[i]);
          const signal = this.loopBody(s.body, frame);
          if (signal?.k === "break") break;
          if (signal) return signal;
        }
        return null;
      }
      default: throw new Error(`model: ${s.k}`);
    }
  }
}

// A program and its expected stdout, redrawn until the model accepts it.
export function generateCase(seed, index) {
  for (let attempt = 0; attempt < ATTEMPTS; attempt++) {
    const program = new Gen(random("jet-safety-fuzz", seed, index, attempt)).program();
    const machine = new Machine(program);
    try {
      machine.exec(program.run, new Env(null));
    } catch (error) {
      if (error instanceof Reject) continue;
      throw error;
    }
    const header = [`Generated by Tools/agent/safety-fuzz.mjs: seed ${seed}, case ${index}, attempt ${attempt}.`];
    return { source: source(program, header), stdout: machine.out.map((line) => `${line}\n`).join("") };
  }
  throw new Error(`seed ${seed} case ${index}: no defined program in ${ATTEMPTS} attempts`);
}

// Stateful Core/runtime API stress, independently modeled without executing Jet.
// Keeps all indexes valid; invalid UTF-8 is a defined fallible decode, not UB.
export function generateCoreCase(seed, index) {
  const rng = random("jet-core-safety-fuzz", seed, index);
  const statements = ["m := [String:Int]{}", "xs := [Int]{}"];
  const output = [];
  const map = new Map();
  const list = [];
  for (let step = 0; step < 64; step++) {
    const key = `k${Math.floor(rng() * 8)}`;
    const value = Math.floor(rng() * 1000);
    switch (step % 4) {
      case 0:
        statements.push(`m["${key}"] = ${value}`, `&xs.push(${value})`);
        map.set(key, value);
        list.push(value);
        break;
      case 1:
        statements.push(`print(m.get("${key}") ?? -1)`, "print(xs.len())");
        output.push(map.get(key) ?? -1, list.length);
        break;
      case 2:
        statements.push(`print((&m.pop("${key}")) ?? -1)`, "print((&xs.pop()) ?? -1)");
        output.push(map.get(key) ?? -1, list.pop() ?? -1);
        map.delete(key);
        break;
      case 3:
        statements.push(`saved${step} :: xs.copy()`, `&xs.push(${value})`, `print(saved${step}.len())`, "print(xs.len())");
        output.push(list.length, list.length + 1);
        list.push(value);
        break;
    }
  }
  const bytes = Array.from({ length: 1 + Math.floor(rng() * 32) }, () => 65 + Math.floor(rng() * 26));
  statements.push(`print(String.from_bytes([U8]{${bytes.join(", ")}}) ?? "bad")`);
  output.push(String.fromCharCode(...bytes));
  for (const invalid of [[255], [128], [226, 130], [192, 175], [237, 160, 128], [244, 144, 128, 128]]) {
    statements.push(`print(String.from_bytes([U8]{${invalid.join(", ")}}) ?? "invalid")`);
    output.push("invalid");
  }
  return {
    source: `// Core API fuzz: seed ${seed}, case ${index}\nfn run() {\n${statements.map((line) => `    ${line}`).join("\n")}\n}\n`,
    stdout: output.map((value) => `${value}\n`).join(""),
  };
}
