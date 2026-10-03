// Core record rows for the Jet compiler: the builtin record field oracle
// (`core_struct_field`, `compiler_package_struct_field`) and the Core struct
// literal oracle (`core_constructable_fields`) projected from the Rust sema
// tables into Compiler/JetFoundation/Source/Registry/CoreRecordRows.jet.
//
// The Rust match tables are the source.  This reader understands the table
// shapes those functions use (type/field guards, `match field` arms, local
// type closures, `Syntax::TYPE_*` names, and nullary `*_ty()` helpers).  A
// Rust statement outside that subset is reported by name and line and keeps
// the projection from being written, so a new table shape cannot silently
// drop rows.

import { createHash } from "node:crypto";
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";

const CORE_TYPES_PATH = "crates/jet-sema/src/Sema/CheckerCoreLib/core_types.rs";
const SYNTAX_DIR = "crates/jet-foundation/src/Syntax";
export const CORE_RECORD_ROWS_PATH = "Compiler/JetFoundation/Source/Registry/CoreRecordRows.jet";
const DIAGNOSTICS_PATH = "crates/jet-sema/src/Sema/Diagnostics.rs";

// Rust rows whose type is computed rather than tabled.  Each one stays a
// Rust-only answer until its table is written as data.
const UNPROJECTED = new Map([
  ["MarkerArgInfo.value", "a union assembled from Policy::RULE_ARG_DECLARATIONS"],
]);

class Unsupported extends Error {
  constructor(message, line) {
    super(message);
    this.line = line;
  }
}

function tokenize(src) {
  const toks = [];
  let i = 0;
  let line = 1;
  const push = (k, v) => toks.push({ k, v, line });
  while (i < src.length) {
    const c = src[i];
    if (c === "\n") { line++; i++; continue; }
    if (/\s/.test(c)) { i++; continue; }
    if (src.startsWith("//", i)) {
      while (i < src.length && src[i] !== "\n") i++;
      continue;
    }
    if (src.startsWith("/*", i)) {
      const end = src.indexOf("*/", i + 2);
      for (let k = i; k < end; k++) if (src[k] === "\n") line++;
      i = end + 2;
      continue;
    }
    if ((c === "r" || c === "b") && /^(br|r|b)?#*"/.test(src.slice(i, i + 4)) && !/[A-Za-z0-9_]/.test(src[i - 1] ?? "")) {
      const m = /^(br|r|b)(#*)"/.exec(src.slice(i));
      if (m) {
        const raw = m[1].includes("r");
        const close = '"' + m[2];
        let j = i + m[0].length;
        let v = "";
        if (raw) {
          const end = src.indexOf(close, j);
          v = src.slice(j, end);
          for (const ch of v) if (ch === "\n") line++;
          i = end + close.length;
        } else {
          while (src[j] !== '"') { if (src[j] === "\\") { v += src[j + 1]; j += 2; } else { if (src[j] === "\n") line++; v += src[j]; j++; } }
          i = j + 1;
        }
        push("str", v);
        continue;
      }
    }
    if (c === '"') {
      let j = i + 1;
      let v = "";
      while (src[j] !== '"') {
        if (src[j] === "\\") { v += src[j + 1]; j += 2; } else { if (src[j] === "\n") line++; v += src[j]; j++; }
      }
      push("str", v);
      i = j + 1;
      continue;
    }
    if (c === "'") {
      if (src[i + 1] === "\\") {
        const end = src.indexOf("'", i + 2);
        push("char", src.slice(i + 1, end));
        i = end + 1;
        continue;
      }
      if (src[i + 2] === "'") { push("char", src[i + 1]); i += 3; continue; }
      let j = i + 1;
      while (/[A-Za-z0-9_]/.test(src[j])) j++;
      push("life", src.slice(i, j));
      i = j;
      continue;
    }
    if (/[A-Za-z_]/.test(c)) {
      let j = i;
      while (j < src.length && /[A-Za-z0-9_]/.test(src[j])) j++;
      push("id", src.slice(i, j));
      i = j;
      continue;
    }
    if (/[0-9]/.test(c)) {
      let j = i;
      while (/[0-9A-Za-z_]/.test(src[j])) j++;
      push("num", src.slice(i, j));
      i = j;
      continue;
    }
    const two = src.slice(i, i + 2);
    if (["::", "=>", "==", "!=", "||", "&&", "->", "..", "<=", ">="].includes(two)) {
      push("p", two);
      i += 2;
      continue;
    }
    push("p", c);
    i++;
  }
  return toks;
}

function cryptoLeaves(root) {
  const text = readFileSync(path.join(root, DIAGNOSTICS_PATH), "utf8");
  const body = /fn secret_bearing_crypto_leaf\(name: &str\) -> bool \{\s*matches!\(\s*name,([^)]*)\)/.exec(text);
  if (!body) throw new Error(`${DIAGNOSTICS_PATH}: secret_bearing_crypto_leaf table not found`);
  return [...body[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
}

function syntaxConstants(root) {
  const constants = new Map();
  const dir = path.join(root, SYNTAX_DIR);
  for (const name of readdirSync(dir)) {
    if (!name.endsWith(".rs")) continue;
    const text = readFileSync(path.join(dir, name), "utf8");
    for (const m of text.matchAll(/pub const (\w+): &str = "([^"]*)";/g)) constants.set(m[1], m[2]);
  }
  return constants;
}

// Function bodies by name: the tokens between the body braces.
function functionBodies(toks) {
  const bodies = new Map();
  for (let i = 0; i + 1 < toks.length; i++) {
    if (toks[i].k !== "id" || toks[i].v !== "fn" || toks[i + 1].k !== "id") continue;
    const name = toks[i + 1].v;
    let j = i + 2;
    let depth = 0;
    const params = [];
    // Parameter names: identifiers followed by `:` at paren depth 1.
    for (; j < toks.length; j++) {
      const t = toks[j];
      if (t.v === "(" ) depth++;
      else if (t.v === ")") { depth--; if (depth === 0) { j++; break; } }
      else if (depth === 1 && t.k === "id" && toks[j + 1]?.v === ":" && toks[j + 1]?.k === "p") params.push(t.v);
    }
    while (j < toks.length && !(toks[j].k === "p" && (toks[j].v === "{" || toks[j].v === ";"))) j++;
    if (toks[j]?.v !== "{") continue;
    const end = matching(toks, j);
    if (!bodies.has(name)) bodies.set(name, { params, toks: toks.slice(j + 1, end) });
  }
  return bodies;
}

function matching(toks, open) {
  const pairs = { "(": ")", "{": "}", "[": "]" };
  const close = pairs[toks[open].v];
  let depth = 0;
  for (let i = open; i < toks.length; i++) {
    if (toks[i].k !== "p") continue;
    if (toks[i].v === toks[open].v) depth++;
    else if (toks[i].v === close) { depth--; if (depth === 0) return i; }
  }
  throw new Error("unbalanced token group");
}

function jetString(value) {
  return JSON.stringify(value).replace(/\{/g, "{{").replace(/\}/g, "}}");
}

class Parser {
  constructor(toks, oracle, env) {
    this.toks = toks;
    this.pos = 0;
    this.oracle = oracle;
    this.env = env;
  }
  peek(offset = 0) { return this.toks[this.pos + offset]; }
  at(v, offset = 0) { const t = this.peek(offset); return t !== undefined && (t.k === "p" || t.k === "id") && t.v === v; }
  done() { return this.pos >= this.toks.length; }
  line() { return (this.peek() ?? this.toks[this.toks.length - 1])?.line ?? 0; }
  next() { return this.toks[this.pos++]; }
  fail(message) { throw new Unsupported(message, this.line()); }
  expect(v) {
    const t = this.next();
    if (!t || t.v !== v) throw new Unsupported(`expected \`${v}\`, found \`${t?.v}\``, t?.line ?? this.line());
    return t;
  }
  eat(v) { if (this.at(v)) { this.pos++; return true; } return false; }
  // Token range up to the next `stop` punctuation at depth 0.
  until(stops) {
    const start = this.pos;
    while (!this.done()) {
      const t = this.peek();
      if (t.k === "p" && stops.includes(t.v)) break;
      if (t.k === "p" && (t.v === "(" || t.v === "{" || t.v === "[")) { this.pos = matching(this.toks, this.pos) + 1; continue; }
      this.pos++;
    }
    return this.toks.slice(start, this.pos);
  }
  group() {
    const open = this.pos;
    const close = matching(this.toks, open);
    this.pos = close + 1;
    return this.toks.slice(open + 1, close);
  }
  sub(toks, env = this.env) { return new Parser(toks, this.oracle, env); }

  // A `String` naming a type: `"X".to_string()`, `Syntax::TYPE_X`, ...
  name() {
    let value;
    const t = this.next();
    if (t?.k === "str") value = t.v;
    else if (t?.k === "id" && t.v === "Syntax" && this.eat("::")) {
      const c = this.next();
      value = this.oracle.syntax.get(c.v);
      if (value === undefined) throw new Unsupported(`unknown Syntax::${c.v}`, c.line);
    } else if (t?.k === "id" && t.v === "String" && this.eat("::")) {
      const f = this.next();
      if (f.v !== "from") throw new Unsupported(`String::${f.v}`, f.line);
      return this.sub(this.group()).name();
    } else if (t?.k === "id" && this.env.has(t.v) && this.env.get(t.v).params === null) {
      value = this.sub(this.env.get(t.v).toks, this.env.get(t.v).env).name();
    } else throw new Unsupported(`type name \`${t?.v}\``, t?.line ?? this.line());
    while (this.at(".")) {
      const method = this.peek(1)?.v;
      if (!["to_string", "to_owned", "into", "clone"].includes(method)) break;
      this.pos += 2;
      this.expect("(");
      this.expect(")");
    }
    return value;
  }

  nameSet() {
    const names = [];
    do names.push(this.name()); while (this.eat("|"));
    return names;
  }

  boxed() {
    if (this.at("Box") && this.at("::", 1) && this.at("new", 2)) {
      this.pos += 3;
      const inner = this.sub(this.group());
      const ty = inner.boxed();
      inner.eat(",");
      if (!inner.done()) inner.fail("trailing tokens in Box::new");
      return ty;
    }
    return this.type();
  }

  vecItems() {
    if (!(this.at("vec") && this.at("!", 1))) this.fail("expected vec![...]");
    this.pos += 2;
    const items = [];
    const inner = this.sub(this.group());
    while (!inner.done()) {
      items.push(inner.until([","]));
      inner.eat(",");
    }
    return items;
  }

  fields(kind) {
    const out = new Map();
    const inner = this.sub(this.group());
    while (!inner.done()) {
      const key = inner.next().v;
      inner.expect(":");
      out.set(key, inner.until([","]));
      inner.eat(",");
    }
    if (kind) for (const key of out.keys()) if (!kind.includes(key)) this.fail(`unexpected field ${key}`);
    return out;
  }

  type() {
    let ty = this.primaryType();
    while (this.at(".") && ["clone", "to_owned"].includes(this.peek(1)?.v)) {
      this.pos += 2;
      this.expect("(");
      this.expect(")");
    }
    return ty;
  }

  primaryType() {
    const t = this.peek();
    if (!t) this.fail("missing type");
    if (t.k === "p" && t.v === "(") {
      const inner = this.sub(this.group());
      const ty = inner.type();
      inner.eat(",");
      if (!inner.done()) inner.fail("trailing tokens");
      return ty;
    }
    if (t.k !== "id") this.fail(`type expression \`${t.v}\``);
    if (t.v === "Type" && this.at("::", 1)) {
      this.pos += 2;
      const variant = this.next().v;
      const single = (label, field) => {
        const inner = this.sub(this.group());
        const ty = inner.boxed();
        inner.eat(",");
        if (!inner.done()) inner.fail(`trailing tokens in Type::${label}`);
        return `Type.${label}{${field}: ${ty}}`;
      };
      switch (variant) {
        case "Int": case "Float": case "Bool": case "String": case "Char": case "Float32":
          return `Type.${variant}`;
        case "List": return single("List", "element");
        case "Option": return single("Option", "inner");
        case "Shared": return single("Shared", "inner");
        case "Named": {
          const inner = this.sub(this.group());
          const name = inner.name();
          inner.eat(",");
          if (!inner.done()) inner.fail("trailing tokens in Type::Named");
          return `Type.Named{name: ${jetString(name)}}`;
        }
        case "IntN": {
          const f = this.fields(["signed", "bits"]);
          const signed = f.get("signed").map((x) => x.v).join("");
          const bits = f.get("bits").map((x) => x.v).join("");
          if (!/^(true|false)$/.test(signed) || !/^\d+$/.test(bits)) this.fail("non-literal IntN");
          return `Type.IntN{signed: ${signed}, bits: ${bits}}`;
        }
        case "Result": {
          if (this.at("(")) {
            const parts = this.sub(this.group());
            const ok = parts.boxed(); parts.expect(","); const err = parts.boxed();
            return `Type.Result{ok: ${ok}, err: ${err}}`;
          }
          const f = this.fields(["ok", "err"]);
          return `Type.Result{ok: ${this.sub(f.get("ok")).boxed()}, err: ${this.sub(f.get("err")).boxed()}}`;
        }
        case "Map": {
          if (this.at("{")) {
            const f = this.fields(["key", "key_span", "value"]);
            if (f.get("key_span").map((x) => x.v).join("") !== "None") this.fail("Map key_span");
            return `Type.Map{key: ${this.sub(f.get("key")).boxed()}, key_span: None, value: ${this.sub(f.get("value")).boxed()}}`;
          }
          this.fail("unsupported Map shape");
        }
        case "Union": {
          const parts = this.sub(this.group());
          const members = parts.vecItems().map((item) => this.sub(item).type());
          return `Type.Union{members: [Type]{${members.join(", ")}}}`;
        }
        case "Tuple": {
          const parts = this.sub(this.group());
          const items = parts.vecItems().map((item) => {
            const pair = this.sub(item);
            const inner = pair.sub(pair.group());
            const name = inner.name();
            inner.expect(",");
            const ty = inner.boxed();
            return `TypeTupleField{name: ${jetString(name)}, ty: ${ty}}`;
          });
          return `Type.Tuple{fields: [TypeTupleField]{${items.join(", ")}}}`;
        }
        case "Apply": {
          const f = this.fields(["name", "args"]);
          const name = this.sub(f.get("name")).name();
          const args = this.sub(f.get("args")).vecItems().map((item) => this.sub(item).type());
          return `Type.Apply{name: ${jetString(name)}, args: [Type]{${args.join(", ")}}}`;
        }
        default:
          this.fail(`Type::${variant}`);
      }
    }
    // `crate::Sema::Diagnostics::core_crypto_nominal(Type::Named(leaf))`.
    if (t.v === "crate" || t.v === "core_crypto_nominal") {
      const start = this.pos;
      while (this.peek()?.k === "id" && this.at("::", 1)) this.pos += 2;
      if (this.next()?.v !== "core_crypto_nominal") { this.pos = start; this.fail("crate path in a type"); }
      const inner = this.sub(this.group());
      const ty = inner.type();
      inner.eat(",");
      const leaf = /^Type\.Named\{name: "([^"]+)"\}$/.exec(ty);
      if (!inner.done() || !leaf) inner.fail("core_crypto_nominal over a non-leaf type");
      if (!this.oracle.cryptoLeaves.includes(leaf[1])) return ty;
      return `Type.Tagged{marker: TagMarker.Internal{internal_tag: InternalTag.CoreCryptoNominal}, inner: ${ty}}`;
    }
    this.pos++;
    const bound = this.env.get(t.v);
    if (bound) {
      if (bound.params === null) return this.sub(bound.toks, bound.env).type();
      const args = this.callArgs();
      if (args.length !== bound.params.length) this.fail(`closure ${t.v} arity`);
      const env = new Map(bound.env);
      bound.params.forEach((param, index) => env.set(param, { params: null, toks: args[index], env: this.env }));
      return this.sub(bound.toks, env).type();
    }
    const helper = this.oracle.bodies.get(t.v);
    if (helper && this.at("(") && this.at(")", 1) && helper.params.length === 0) {
      this.pos += 2;
      return this.oracle.helperType(t.v);
    }
    this.pos--;
    this.fail(`type expression \`${t.v}\``);
  }

  callArgs() {
    const inner = this.sub(this.group());
    const args = [];
    while (!inner.done()) { args.push(inner.until([","])); inner.eat(","); }
    return args;
  }
}

// Rows collected in Rust first-match order.
class Rows {
  constructor() {
    this.rows = [];
    this.seen = new Set();
    this.closed = new Set();
  }
  add(types, fields, ty) {
    for (const type of types) {
      if (this.closed.has(type)) continue;
      for (const field of fields) {
        const key = type + "\u0000" + field;
        if (this.seen.has(key)) continue;
        this.seen.add(key);
        this.rows.push({ type, field, ty });
      }
    }
  }
  close(types) { for (const type of types) this.closed.add(type); }
}

class FieldReader {
  constructor(oracle, rows, typeVar, fieldVar) {
    this.oracle = oracle;
    this.rows = rows;
    this.typeVar = typeVar;
    this.fieldVar = fieldVar;
  }

  // Returns true when the block always returns.
  block(p, types, fields) {
    while (!p.done()) {
      if (p.eat(";")) continue;
      if (p.at("let")) { this.letBinding(p); continue; }
      if (p.at("if")) {
        const returned = this.ifStatement(p, types, fields);
        if (returned === "terminal") return true;
        continue;
      }
      if (p.at("return")) {
        p.next();
        this.result(p.sub(p.until([";"]), p.env), types, fields);
        p.eat(";");
        return true;
      }
      if (p.at("match") && this.isMatchStatement(p)) {
        p.next();
        const scrutinee = p.until(["{"]);
        const terminal = this.matchArms(p, scrutinee, p.group(), types, fields, true);
        if (terminal) return true;
        continue;
      }
      // Tail expression.
      const expr = p.sub(p.until([";"]), p.env);
      if (p.done()) {
        this.result(expr, types, fields);
        return true;
      }
      p.fail(`statement \`${expr.toks[0]?.v}\``);
    }
    return false;
  }

  isMatchStatement(p) {
    // `match ... { ... }` followed by more statements.
    const save = p.pos;
    p.next();
    p.until(["{"]);
    p.group();
    const statement = !p.done();
    p.pos = save;
    return statement;
  }

  letBinding(p) {
    p.expect("let");
    const name = p.next();
    if (name.k !== "id" || !p.at("=")) p.fail("unsupported let pattern");
    p.expect("=");
    let params = null;
    if (p.eat("||")) params = [];
    else if (p.eat("|")) {
      params = [];
      while (!p.eat("|")) {
        params.push(p.next().v);
        p.until([",", "|"]);
        p.eat(",");
      }
    }
    const toks = p.until([";"]);
    p.eat(";");
    p.env.set(name.v, { params, toks, env: p.env });
  }

  // `if COND { ... }` at statement level; `else` chains are not table shapes.
  ifStatement(p, types, fields) {
    p.expect("if");
    if (p.at("let")) {
      p.next();
      if (!(p.at("Some") && p.at("(", 1))) p.fail("if let shape");
      p.next();
      p.group();
      p.expect("=");
      const fn = p.next().v;
      const args = p.callArgs().map((arg) => arg.map((x) => x.v).join(""));
      p.group();
      if (args.join(",") !== `${this.typeVar},${this.fieldVar}`) p.fail(`if let ${fn}(...)`);
      const body = this.oracle.bodies.get(fn);
      if (!body) p.fail(`unknown fn ${fn}`);
      const reader = new FieldReader(this.oracle, this.rows, body.params[0], body.params[1]);
      reader.block(new Parser(body.toks, this.oracle, new Map()), types, fields);
      return "open";
    }
    const cond = this.cond(p.sub(p.until(["{"]), p.env));
    const body = p.sub(p.group(), new Map(p.env));
    if (p.at("else")) p.fail("if/else in a record table");
    const narrowedTypes = cond.types ?? types;
    const narrowedFields = cond.fields ?? fields;
    const terminal = this.block(body, narrowedTypes, narrowedFields);
    if (terminal && cond.fields === null && cond.types !== null) this.rows.close(cond.types);
    return "open";
  }

  // Conditions over the type and field names.
  cond(p) {
    const parts = [this.conj(p)];
    while (p.eat("||")) parts.push(this.conj(p));
    if (!p.done()) p.fail(`condition near \`${p.peek().v}\``);
    if (parts.length === 1) return parts[0];
    if (parts.some((part) => part.fields !== null || part.types === null)) p.fail("disjunction over fields");
    return { types: parts.flatMap((part) => part.types), fields: null };
  }

  conj(p) {
    const out = { types: null, fields: null };
    do {
      const atom = this.atom(p);
      if (atom.types) out.types = atom.types;
      if (atom.fields) out.fields = atom.fields;
    } while (p.eat("&&"));
    return out;
  }

  atom(p) {
    const t = p.next();
    if (t.k === "id" && (t.v === this.typeVar || t.v === this.fieldVar) && p.eat("==")) {
      const names = [p.name()];
      return t.v === this.typeVar ? { types: names, fields: null } : { types: null, fields: names };
    }
    if (t.k === "id" && t.v === "matches" && p.eat("!")) {
      const inner = p.sub(p.group());
      const subject = inner.next().v;
      inner.expect(",");
      const names = inner.nameSet();
      if (subject === this.typeVar) return { types: names, fields: null };
      if (subject === this.fieldVar) return { types: null, fields: names };
      p.fail(`matches!(${subject}, ...)`);
    }
    if (t.k === "id" && p.at("(")) {
      const args = p.callArgs().map((arg) => arg.map((x) => x.v).join(""));
      const body = this.oracle.bodies.get(t.v);
      if (body && args.length === 1 && args[0] === this.typeVar) {
        const reader = new FieldReader(this.oracle, this.rows, body.params[0], "\u0000");
        return reader.cond(new Parser(body.toks, this.oracle, new Map()));
      }
    }
    p.pos--;
    p.fail(`condition \`${t.v}\``);
  }

  // A value the table returns: `Some(T)`, `None`, a field match, ...
  result(p, types, fields) {
    if (p.at("None") && p.toks.length === 1) return;
    if (p.at("Some") && p.at("(", 1)) {
      p.next();
      const inner = p.sub(p.group());
      if (!p.done()) p.fail("trailing tokens after Some(...)");
      const ty = inner.type();
      if (!inner.done()) inner.fail("trailing tokens in Some(...)");
      if (types === null || fields === null) p.fail("row without a type and field");
      this.rows.add(types, fields, ty);
      return;
    }
    if (p.at("match")) {
      p.next();
      const scrutinee = p.until(["{"]);
      this.matchArms(p, scrutinee, p.group(), types, fields, false);
      if (!p.done()) p.fail("trailing tokens after match");
      return;
    }
    if (p.at("matches") && p.at("!", 1)) {
      p.pos += 2;
      const inner = p.sub(p.group());
      if (inner.next().v !== this.fieldVar) inner.fail("matches! subject");
      inner.expect(",");
      const names = inner.nameSet();
      p.expect(".");
      if (p.next().v !== "then_some") p.fail("matches!(...).then_some");
      const value = p.sub(p.group());
      const ty = value.type();
      this.rows.add(types, names, ty);
      return;
    }
    if (p.at("{")) {
      const body = p.sub(p.group(), new Map(p.env));
      if (!p.done()) p.fail("trailing tokens after block");
      this.block(body, types, fields);
      return;
    }
    p.fail(`result \`${p.peek()?.v}\``);
  }

  // Returns true when every arm returns (a match used as a statement with a
  // wildcard arm that returns).
  matchArms(p, scrutineeToks, armToks, types, fields, statement) {
    const scrutinee = scrutineeToks.map((x) => x.v).join(" ");
    const arms = p.sub(armToks, p.env);
    let exhaustiveReturn = false;
    while (!arms.done()) {
      const pattern = arms.sub(arms.until(["=>"]), arms.env);
      arms.expect("=>");
      let value;
      if (arms.at("{")) { value = arms.sub(arms.group(), new Map(arms.env)); arms.eat(","); }
      else { value = arms.sub(arms.until([","]), new Map(arms.env)); arms.eat(","); }
      // Each alternative is one (types, fields) pair the value answers.
      let alts = [[types, fields]];
      let wildcard = false;
      if (scrutinee === this.typeVar) {
        const names = this.patternNames(pattern);
        if (names === null) wildcard = true; else alts = [[names, fields]];
      } else if (scrutinee === this.fieldVar) {
        const names = this.patternNames(pattern);
        if (names === null) wildcard = true; else alts = [[types, names]];
      } else if (scrutinee === `( ${this.typeVar} , ${this.fieldVar} )`) {
        if (pattern.at("_") && pattern.toks.length === 1) wildcard = true;
        else {
          alts = [];
          do {
            const tuple = pattern.sub(pattern.group(), pattern.env);
            const typePart = tuple.sub(tuple.until([","]), tuple.env);
            tuple.expect(",");
            const fieldPart = tuple.sub(tuple.until([","]), tuple.env);
            alts.push([this.patternNames(typePart) ?? types, this.patternNames(fieldPart) ?? fields]);
          } while (pattern.eat("|"));
          if (!pattern.done()) pattern.fail("tuple arm guard");
        }
      } else {
        p.fail(`match on \`${scrutinee}\``);
      }
      const isEmpty = value.toks.length === 0;
      if (wildcard) {
        if (isEmpty) continue;
        if (value.at("None") && value.toks.length === 1) { exhaustiveReturn = true; continue; }
        // `_ => core_constructable_fields(type_name)?...find(field)`: every
        // type still open here answers from the struct literal table.
        if (value.at("core_constructable_fields") && types === null && fields === null) {
          this.oracle.constructableFallback = true;
          exhaustiveReturn = true;
          continue;
        }
      }
      if (isEmpty) continue;
      for (const [armTypes, armFields] of alts) {
        const armValue = value.sub(value.toks, new Map(value.env));
        try {
          if (statement && !armValue.toks.some((x) => x.v === "return")) {
            armValue.fail("statement match arm without return");
          }
          if (statement) {
            const terminal = this.block(armValue, armTypes, armFields);
            if (terminal && scrutinee === this.typeVar && armFields === fields && fields === null) this.rows.close(armTypes);
          } else if (armValue.at("let")) {
            this.block(armValue, armTypes, armFields);
          } else {
            this.result(armValue, armTypes, armFields);
          }
        } catch (error) {
          if (!(error instanceof Unsupported)) throw error;
          const key = `${(armTypes ?? ["?"]).join("|")}.${(armFields ?? ["?"]).join("|")}`;
          if (!UNPROJECTED.has(key)) this.oracle.armSkips.push(`${key} (core_types.rs:${error.line}): ${error.message}`);
        }
      }
    }
    return exhaustiveReturn;
  }

  // `"a" | "b"`, `Syntax::X`, `name if name == X || name == Y`, or `_` (null).
  patternNames(p) {
    if (p.at("_") && p.toks.length === 1) return null;
    const first = p.peek();
    if (first.k === "id" && first.v !== "Syntax" && p.at("if", 1)) {
      p.pos += 2;
      const names = [];
      do {
        if (p.next().v !== first.v) p.fail("pattern guard");
        p.expect("==");
        names.push(p.name());
      } while (p.eat("||"));
      if (!p.done()) p.fail("pattern guard");
      return names;
    }
    const names = p.nameSet();
    if (!p.done()) p.fail(`pattern near \`${p.peek().v}\``);
    return names;
  }
}

function createOracle(root) {
  const source = readFileSync(path.join(root, CORE_TYPES_PATH), "utf8");
  const toks = tokenize(source);
  const oracle = {
    source,
    syntax: syntaxConstants(root),
    bodies: functionBodies(toks),
    cryptoLeaves: cryptoLeaves(root),
    armSkips: [],
    helperType(name) {
      const body = this.bodies.get(name);
      const p = new Parser(body.toks, this, new Map());
      while (p.at("let")) new FieldReader(this, new Rows(), "", "").letBinding(p);
      const ty = p.type();
      if (!p.done()) p.fail(`helper ${name} body`);
      return ty;
    },
  };
  return oracle;
}

function fieldRows(oracle, skipped) {
  const rows = new Rows();
  const body = oracle.bodies.get("core_struct_field");
  const reader = new FieldReader(oracle, rows, body.params[0], body.params[1]);
  const p = new Parser(body.toks, oracle, new Map());
  // Each top-level statement is read on its own so one unrepresentable
  // shape is named precisely.
  while (!p.done()) {
    const start = p.pos;
    try {
      if (p.eat(";")) continue;
      if (p.at("if")) { reader.ifStatement(p, null, null); continue; }
      if (p.at("let")) { reader.letBinding(p); continue; }
      if (p.at("match")) {
        p.next();
        const scrutinee = p.until(["{"]);
        const arms = p.group();
        reader.matchArms(p, scrutinee, arms, null, null, !p.done());
        continue;
      }
      if (p.at("None") && p.toks.length === p.pos + 1) { p.next(); continue; }
      p.fail(`statement \`${p.peek().v}\``);
    } catch (error) {
      if (!(error instanceof Unsupported)) throw error;
      skipped.push(`core_struct_field:${error.line}: ${error.message}`);
      p.pos = start;
      skipStatement(p);
    }
  }
  return rows;
}

function skipStatement(p) {
  // Skip one statement: up to the end of its first brace group or `;`.
  while (!p.done()) {
    const t = p.peek();
    if (t.k === "p" && t.v === ";") { p.pos++; return; }
    if (t.k === "p" && t.v === "{") {
      p.pos = matching(p.toks, p.pos) + 1;
      if (!p.at("else")) { p.eat(";"); return; }
      continue;
    }
    if (t.k === "p" && (t.v === "(" || t.v === "[")) { p.pos = matching(p.toks, p.pos) + 1; continue; }
    p.pos++;
  }
}

function literalRows(oracle, skipped) {
  const body = oracle.bodies.get("core_constructable_fields");
  const typeVar = body.params[0];
  const p = new Parser(body.toks, oracle, new Map());
  const reader = new FieldReader(oracle, new Rows(), typeVar, "\u0000");
  while (p.at("let")) reader.letBinding(p);
  p.expect("match");
  if (p.next().v !== typeVar) p.fail("core_constructable_fields scrutinee");
  const arms = p.sub(p.group(), p.env);
  const records = [];
  const seen = new Set();
  while (!arms.done()) {
    const pattern = arms.sub(arms.until(["=>"]), arms.env);
    arms.expect("=>");
    const line = arms.line();
    const value = arms.sub(arms.until([","]), new Map(arms.env));
    arms.eat(",");
    try {
      const names = reader.patternNames(pattern);
      if (names === null) continue;
      let v = value;
      if (v.at("{")) {
        v = v.sub(v.group(), new Map(v.env));
        while (v.at("let")) reader.letBinding(v);
      }
      if (v.at("None") && v.toks.length - v.pos === 1) {
        for (const name of names) seen.add(name);
        continue;
      }
      if (!(v.at("Some") && v.at("(", 1))) v.fail("constructable arm value");
      v.next();
      const inner = v.sub(v.group());
      const fields = inner.vecItems().map((item) => {
        const pair = inner.sub(item);
        const tuple = pair.sub(pair.group());
        const name = tuple.name();
        tuple.expect(",");
        const ty = tuple.type();
        tuple.eat(",");
        if (!tuple.done()) tuple.fail("constructable field tuple");
        return { name, ty };
      });
      for (const name of names) {
        if (seen.has(name)) continue;
        seen.add(name);
        records.push({ type: name, fields });
      }
    } catch (error) {
      if (!(error instanceof Unsupported)) throw error;
      skipped.push(`core_constructable_fields:${error.line ?? line}: ${error.message}`);
    }
  }
  return records;
}

export function generatedCoreRecordRows(root) {
  const oracle = createOracle(root);
  const skipped = [];
  const fieldTable = fieldRows(oracle, skipped);
  const literals = literalRows(oracle, skipped);
  if (!oracle.constructableFallback) skipped.push("core_struct_field: the constructable-field fallback arm is missing");
  for (const record of literals) {
    for (const field of record.fields) fieldTable.add([record.type], [field.name], field.ty);
  }
  const fields = fieldTable.rows;
  skipped.push(...oracle.armSkips);
  if (skipped.length) {
    throw new Error(
      "CoreRecordRows: Rust record table shapes this projection cannot read:\n  " + skipped.join("\n  "),
    );
  }
  const lines = [
    "// BEGIN GENERATED CORE RECORD ROWS",
    "// Source: " + CORE_TYPES_PATH,
    "// Source SHA-256: " + createHash("sha256").update(oracle.source).digest("hex"),
    "// Generated by Tools/agent/gen-core-tables.mjs --write; do not edit.",
    "// Field rows are the Rust `core_struct_field` oracle in first-match order;",
    "// literal rows are `core_constructable_fields`, in declaration order.",
    "",
    "pub CORE_RECORD_FIELD_ROWS :: [CoreRecordFieldRow]{",
  ];
  for (const row of fields) {
    lines.push(`    CoreRecordFieldRow{owner: ${jetString(row.type)}, field: ${jetString(row.field)}, ty: ${row.ty}},`);
  }
  lines.push("}", "", "pub CORE_RECORD_LITERAL_ROWS :: [CoreRecordLiteralRow]{");
  for (const record of literals) {
    const fieldsText = record.fields
      .map((field) => `TypeTupleField{name: ${jetString(field.name)}, ty: ${field.ty}}`)
      .join(", ");
    lines.push(`    CoreRecordLiteralRow{owner: ${jetString(record.type)}, fields: [TypeTupleField]{${fieldsText}}},`);
  }
  lines.push("}", "// END GENERATED CORE RECORD ROWS", "");
  return lines.join("\n");
}
