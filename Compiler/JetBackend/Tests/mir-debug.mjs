// Real MIR for the Jet backend harness: the Rust compiler's checked MIR
// (written in Rust Debug form when JET_DUMP_MIR=<path> is set, see
// crates/jet-codegen/src/Codegen/TIR/mir.rs) converted into the Jet MIR
// schema (Compiler/JetFoundation/Source/MIR/MIR.jet).
//
// The conversion follows the field and variant correspondence of the
// bootstrap codec (Compiler/Bootstrap/Host/RuntimeMirCodec.rs): types are
// matched by the Jet declaration, fields and variants by name (acronyms
// compared case-insensitively), and a Rust tuple, newtype or map entry fills
// a Jet struct or variant by position. Checked derived facts follow the
// canonical bootstrap codec; absent required facts reject a stale dump.
//
// The converted program is written as a line stream that a decoder generated
// from the same schema (`jetDecoder`) reads back in Jet: one token per line,
// integers in decimal, strings as `s<text>` or `x<hex>`, lists and maps as a
// count then their items, options as 0 or 1 then the value, enums as the
// variant index then the payload fields.
import { readFileSync } from "node:fs";

// ---------------------------------------------------------------------------
// Rust Debug parser.

export function parseRustDebug(text) {
  let i = 0;
  const n = text.length;
  const fail = (what) => {
    throw new Error(`Rust Debug parse: ${what} at offset ${i}: ${JSON.stringify(text.slice(i, i + 60))}`);
  };
  const space = () => {
    while (i < n && /[\s,]/.test(text[i])) i++;
  };
  const ident = () => {
    const start = i;
    while (i < n && (/[A-Za-z0-9_]/.test(text[i]) || (text[i] === ":" && text[i + 1] === ":" && /[A-Za-z_]/.test(text[i + 2] ?? "")))) i += text[i] === ":" ? 2 : 1;
    return text.slice(start, i);
  };
  const quoted = (close) => {
    i++;
    let out = "";
    while (i < n && text[i] !== close) {
      if (text[i] === "\\") {
        const e = text[i + 1];
        i += 2;
        if (e === "n") out += "\n";
        else if (e === "r") out += "\r";
        else if (e === "t") out += "\t";
        else if (e === "0") out += "\0";
        else if (e === "u") {
          const end = text.indexOf("}", i);
          out += String.fromCodePoint(parseInt(text.slice(i + 1, end), 16));
          i = end + 1;
        } else out += e;
      } else {
        const cp = text.codePointAt(i);
        out += String.fromCodePoint(cp);
        i += cp > 0xffff ? 2 : 1;
      }
    }
    i++;
    return out;
  };
  const items = (close) => {
    const out = [];
    space();
    while (text[i] !== close) {
      out.push(value());
      space();
      if (i >= n) fail("unterminated sequence");
    }
    i++;
    return out;
  };
  function value() {
    space();
    const c = text[i];
    if (c === '"') return { k: "str", v: quoted('"') };
    if (c === "'") return { k: "char", v: quoted("'") };
    if (c === "[") {
      i++;
      return { k: "list", items: items("]") };
    }
    if (c === "(") {
      i++;
      return { k: "tuple", name: null, items: items(")") };
    }
    if (c === "{") {
      // A map `{k: v}` or a set `{a, b}`.
      i++;
      const entries = [];
      const set = [];
      space();
      while (text[i] !== "}") {
        const key = value();
        space();
        if (text[i] === ":" && text[i + 1] !== ":") {
          i++;
          entries.push([key, value()]);
        } else set.push(key);
        space();
        if (i >= n) fail("unterminated map");
      }
      i++;
      return set.length > 0 && entries.length === 0 ? { k: "list", items: set } : { k: "map", entries };
    }
    if (/[-0-9]/.test(c)) {
      const start = i;
      i++;
      while (i < n && /[0-9a-zA-Z_.+\-]/.test(text[i]) && !(text[i] === "-" && !/[eE]/.test(text[i - 1]))) i++;
      const raw = text.slice(start, i);
      if (raw === "-inf") return { k: "num", v: raw };
      return { k: "num", v: raw };
    }
    if (c === "<") {
      // `<native-owned>` and similar opaque Debug forms.
      const end = text.indexOf(">", i);
      const raw = text.slice(i, end + 1);
      i = end + 1;
      return { k: "unit", name: raw };
    }
    const name = ident();
    if (name === "") fail("unexpected character");
    if (name === "true" || name === "false") return { k: "bool", v: name === "true" };
    if (name === "NaN" || name === "inf") return { k: "num", v: name };
    const save = i;
    while (i < n && text[i] === " ") i++;
    if (text[i] === "{") {
      i++;
      const fields = [];
      space();
      while (text[i] !== "}") {
        const field = ident();
        space();
        if (text[i] !== ":") fail(`expected ':' after field ${field}`);
        i++;
        fields.push([field, value()]);
        space();
        if (i >= n) fail("unterminated struct");
      }
      i++;
      return { k: "struct", name, fields };
    }
    if (text[i] === "(") {
      i++;
      return { k: "tuple", name, items: items(")") };
    }
    i = save;
    return { k: "unit", name };
  }
  const result = value();
  return result;
}

// ---------------------------------------------------------------------------
// Jet schema.

function parseType(raw) {
  const t = raw.trim().replace(/,$/, "").trim();
  if (t.endsWith("?")) return { k: "opt", of: parseType(t.slice(0, -1)) };
  if (t.startsWith("[") && t.endsWith("]")) {
    const inner = t.slice(1, -1);
    let depth = 0;
    for (let j = 0; j < inner.length; j++) {
      const ch = inner[j];
      if (ch === "[" || ch === "(") depth++;
      else if (ch === "]" || ch === ")") depth--;
      else if (ch === ":" && depth === 0) return { k: "map", key: parseType(inner.slice(0, j)), value: parseType(inner.slice(j + 1)) };
    }
    return { k: "list", of: parseType(inner) };
  }
  return { k: "named", name: t };
}

function splitTopLevel(text, sep) {
  const out = [];
  let depth = 0;
  let start = 0;
  for (let j = 0; j < text.length; j++) {
    const ch = text[j];
    if (ch === "[" || ch === "(" || ch === "{") depth++;
    else if (ch === "]" || ch === ")" || ch === "}") depth--;
    else if (ch === sep && depth === 0) {
      out.push(text.slice(start, j));
      start = j + 1;
    }
  }
  out.push(text.slice(start));
  return out.map((s) => s.trim()).filter((s) => s !== "");
}

// Struct and enum declarations of one Jet source, by name.
export function parseJetSchema(source, only = null) {
  const defs = new Map();
  const clean = source.replace(/\/\/[^\n]*/g, "");
  const re = /^(?:pub )?(struct|enum) (\w+)\s*\{/gm;
  let m;
  while ((m = re.exec(clean))) {
    const [, kind, name] = m;
    let depth = 1;
    let j = re.lastIndex;
    while (depth > 0) {
      if (clean[j] === "{") depth++;
      else if (clean[j] === "}") depth--;
      j++;
    }
    const body = clean.slice(re.lastIndex, j - 1);
    if (only && !only.includes(name)) continue;
    if (kind === "struct") {
      const fields = [];
      for (const fm of body.matchAll(/pub (\w+): ([^\n]+?)\s*(?=\n|pub |$)/g)) fields.push({ name: fm[1], type: parseType(fm[2]) });
      defs.set(name, { kind, name, fields });
    } else {
      const variants = [];
      let k = 0;
      while (k < body.length) {
        while (k < body.length && /\s/.test(body[k])) k++;
        if (k >= body.length) break;
        const start = k;
        while (k < body.length && /\w/.test(body[k])) k++;
        const vname = body.slice(start, k);
        if (vname === "") throw new Error(`schema: cannot read variant of ${name} near ${body.slice(k, k + 40)}`);
        const fields = [];
        if (body[k] === "(") {
          let d = 1;
          const ps = k + 1;
          k++;
          while (d > 0) {
            if (body[k] === "(") d++;
            else if (body[k] === ")") d--;
            k++;
          }
          splitTopLevel(body.slice(ps, k - 1), ",").forEach((part, index) => {
            const colon = part.indexOf(":");
            if (colon >= 0 && /^\w+$/.test(part.slice(0, colon).trim())) fields.push({ name: part.slice(0, colon).trim(), type: parseType(part.slice(colon + 1)) });
            else fields.push({ name: `_${index}`, type: parseType(part) });
          });
        }
        variants.push({ name: vname, fields });
      }
      defs.set(name, { kind, name, variants });
    }
  }
  return defs;
}

// The Jet MIR schema exactly as the harness unit assembles it: MIR.jet plus
// the support declarations the bootstrap codec names (SUPPORT_ITEMS).
export const SUPPORT_ITEMS = [
  ["Compiler/JetFoundation/Source/Diagnostics/Diagnostic.jet", ["Span"]],
  ["Compiler/JetFoundation/Source/Registry/CoreCalls.jet", ["Effect", "SinkClass", "CoreCallFallibility", "CoreCallPureRoute", "CoreCallInterpreterRoute", "CoreCallSymbol", "CoreMarkerApplication"]],
  ["Compiler/JetFoundation/Source/Types/Types.jet", ["ParamZone"]],
  ["Compiler/JetFoundation/Source/Target/Layout.jet", ["ByteLayout", "FieldLayoutFacts", "LayoutFacts"]],
];

export function loadMirSchema(repo) {
  const read = (p) => readFileSync(`${repo}/${p}`, "utf8");
  const defs = parseJetSchema(read("Compiler/JetFoundation/Source/MIR/MIR.jet"));
  for (const [path, names] of SUPPORT_ITEMS) {
    for (const [name, def] of parseJetSchema(read(path), names)) defs.set(name, def);
  }
  return defs;
}

const PRIMITIVES = new Set(["Int", "Float", "Bool", "String", "U8", "U16", "U32", "U64", "I8", "I16", "I32", "I64", "Char"]);

// ---------------------------------------------------------------------------
// Rust -> Jet conversion. A converted value is a plain JS tree:
// Int/Float -> {int: "<decimal>"}, String -> string, Bool -> boolean,
// list -> array, option -> {opt: value|null}, map -> {map: [[k, v]]},
// struct -> {fields: [...]} in declaration order, enum -> {tag, fields}.

// Field renames of the bootstrap codec (`source_field_name`): Rust name -> Jet name.
const FIELD_RENAMES = {
  MIRImport: { module: "module_id" },
  MIRTypeDef: { module: "module_id" },
  MIRFunction: { module: "module_name" },
  MIRPreludeCall: { module: "module_name", effect: "effect_kind" },
  MIRCoreCall: { module: "module_name", effect: "effect_kind" },
  MIRForeign: { module: "module_name" },
  MIRCLib: { module: "module_name" },
  MIRTraitDef: { module: "module_id" },
  MIRImplDef: { module: "module_id" },
  MIRConstantDef: { module: "module_id" },
  MIRNameDeclaration: { module: "module_index" },
  MIRNameAlias: { module: "module_index" },
  MIRLocal: { comptime: "is_comptime" },
};

// Variant renames: Jet enum -> Rust variant name -> Jet variant name.
const VARIANT_RENAMES = {
  MIRStringPartKind: { Value: "Interpolation" },
};

const fold = (name) => name.toLowerCase().replace(/_/g, "");

const rustField = (node, name) => node?.k === "struct" ? node.fields.find(([key]) => key === name)?.[1] : undefined;
const rustId = (node) => node?.k === "tuple" && node.items.length === 1 ? rustId(node.items[0]) : node?.k === "num" ? node.v : null;


export class Converter {
  constructor(schema) {
    this.schema = schema;
    this.trapRoutes = null;
  }

  convert(node, type, where) {
    if (type.k === "opt") {
      if (node.k === "unit" && node.name === "None") return { opt: null };
      if (node.k === "tuple" && node.name === "Some" && node.items.length === 1) return { opt: this.convert(node.items[0], type.of, where) };
      return { opt: this.convert(node, type.of, where) };
    }
    if (node.k === "tuple" && node.name === "Some" && node.items.length === 1) return this.convert(node.items[0], type, where);
    if (type.k === "list") {
      if (node.k === "unit" && node.name === "None") return [];
      if (node.k === "list") return node.items.map((item) => this.convert(item, type.of, where));
      if (node.k === "map") return node.entries.map(([key, value]) => this.convert({ k: "tuple", name: null, items: [key, value] }, type.of, where));
      if (node.k === "tuple" && node.name === null) return node.items.map((item) => this.convert(item, type.of, where));
      throw new Error(`${where}: expected a list, got ${node.k} ${node.name ?? ""}`);
    }
    if (type.k === "map") {
      if (node.k === "map") return { map: node.entries.map(([key, value]) => [this.convert(key, type.key, where), this.convert(value, type.value, where)]) };
      if (node.k === "list") return { map: node.items.map((item) => [this.convert(item.items[0], type.key, where), this.convert(item.items[1], type.value, where)]) };
      throw new Error(`${where}: expected a map, got ${node.k}`);
    }
    const name = type.name;
    if (PRIMITIVES.has(name)) return this.primitive(node, name, where);
    const def = this.schema.get(name);
    if (!def) throw new Error(`schema has no type ${name}`);
    return def.kind === "struct" ? this.struct(node, def, where) : this.enumValue(node, def, where);
  }

  primitive(node, name, where) {
    // Newtypes (`MirTypeId(5)`) and boxes print as one-item tuples.
    if (node.k === "tuple" && node.items.length === 1) return this.primitive(node.items[0], name, where);
    if (name === "String") {
      if (node.k === "str" || node.k === "char") return node.v;
      if (node.k === "unit") return node.name;
      // Byte arrays (`[u8; 32]` digests) are carried as lowercase hex text.
      if (node.k === "list" && node.items.every((item) => item.k === "num")) return node.items.map((item) => Number(item.v).toString(16).padStart(2, "0")).join("");
      throw new Error(`${where}: expected a String, got ${node.k}`);
    }
    if (name === "Bool") {
      if (node.k === "bool") return node.v;
      throw new Error(`${where}: expected a Bool, got ${node.k}`);
    }
    if (name === "Char") {
      if (node.k === "char") return { int: String(node.v.codePointAt(0)) };
    }
    if (name === "Float") {
      if (node.k !== "num") throw new Error(`${where}: expected a Float, got ${node.k}`);
      return { int: floatBits(node.v) };
    }
    if (node.k === "char") return { int: String(node.v.codePointAt(0)) };
    if (node.k === "bool") return { int: node.v ? "1" : "0" };
    if (node.k !== "num") throw new Error(`${where}: expected an integer, got ${node.k} ${node.name ?? ""}`);
    if (!/^-?[0-9]+$/.test(node.v)) throw new Error(`${where}: ${node.v} is not an integer`);
    return { int: BigInt(node.v).toString() };
  }

  struct(node, def, where) {
    // The canonical bootstrap codec splits Option<Option<usize>> into a
    // finite bound and an unbounded bit; None is not the same as Some(None).
    if (def.name === "MIRFunction" && node.k === "struct") {
      const memo = rustField(node, "memo_bound");
      if (!memo) throw new Error(`${where}: MIRFunction has no checked memo bound`);
      let bound = memo;
      let unbounded = false;
      if (memo.k === "tuple" && memo.name === "Some") {
        const inner = memo.items[0];
        if (inner?.k === "unit" && inner.name === "None") { bound = inner; unbounded = true; }
        else if (inner?.k === "tuple" && inner.name === "Some") bound = inner;
        else throw new Error(`${where}: memo bound is not Option<Option<usize>>`);
      }
      node = { ...node, fields: node.fields.filter(([name]) => name !== "memo_unbounded").map(([name, value]) => [name, name === "memo_bound" ? bound : value]).concat([["memo_unbounded", { k: "bool", v: unbounded }]]) };
    }
    if (def.name === "MIRProgram" && node.k === "struct") {
      this.trapRoutes = new Set();
      for (const row of rustField(node, "prelude_calls")?.items ?? []) {
        const family = rustField(row, "family");
        const member = rustField(row, "member");
        if (family?.name === "Overflow" && /^(?:i|u)(?:8|16|32|64)\.trap\.(?:add|sub|mul|div|pow|shl|shr)$/.test(member?.v ?? "")) this.trapRoutes.add(rustId(rustField(row, "id")));
      }
    }
    const at = `${where}.${def.name}`;
    if (node.k === "struct") {
      const renames = FIELD_RENAMES[def.name] ?? {};
      const host = new Map();
      for (const [field, value] of node.fields) host.set(fold(renames[field] ?? field), value);
      const named = def.fields.every((f) => host.has(fold(f.name)));
      if (!named && node.fields.length === def.fields.length && def.fields.every((f) => !host.has(fold(f.name)))) {
        return { fields: def.fields.map((f, index) => this.convert(node.fields[index][1], f.type, `${at}.${f.name}`)) };
      }
      return {
        fields: def.fields.map((f) => {
          const value = host.get(fold(f.name));
          if (value === undefined) {
            throw new Error(`${def.name}.${f.name}: missing checked Rust field; regenerate the MIR dump with the current compiler`);
          }
          return this.convert(value, f.type, `${at}.${f.name}`);
        }),
      };
    }
    // A Rust enum value filling a Jet struct that wraps that enum
    // (`MirStringPart::Value(v)` -> `MIRStringPart{kind: Interpolation(v)}`).
    const wrapped = def.fields.length === 1 && def.fields[0].type.k === "named" ? this.schema.get(def.fields[0].type.name) : undefined;
    if (wrapped?.kind === "enum" && (node.k === "tuple" || node.k === "unit") && node.name && fold(node.name) !== fold(def.name)) {
      return { fields: [this.convert(node, def.fields[0].type, `${at}.${def.fields[0].name}`)] };
    }
    if (node.k === "tuple" || node.k === "list") {
      const items = node.items;
      if (def.fields.length === 1 && items.length !== 1) return { fields: [this.convert(node, def.fields[0].type, `${at}.${def.fields[0].name}`)] };
      if (items.length !== def.fields.length) throw new Error(`${def.name}: ${items.length} positional values for ${def.fields.length} checked fields`);
      return { fields: def.fields.map((f, index) => this.convert(items[index], f.type, `${at}.${f.name}`)) };
    }
    if (node.k === "unit") {
      if (def.fields.length > 0) throw new Error(`${def.name}: unit value ${node.name} cannot supply checked fields`);
      return { fields: [] };
    }
    if (def.fields.length === 1) return { fields: [this.convert(node, def.fields[0].type, `${at}.${def.fields[0].name}`)] };
    throw new Error(`${at}: cannot fill a struct from ${node.k}`);
  }

  enumValue(node, def, where) {
    const at = `${where}.${def.name}`;
    const hostName = VARIANT_RENAMES[def.name]?.[node.name ?? ""] ?? node.name ?? "";
    let tag = def.variants.findIndex((v) => fold(v.name) === fold(hostName));
    // Rust spells "no such fact" `None` where Jet names the variant `No<Thing>`.
    if (tag < 0 && hostName === "None") tag = def.variants.findIndex((v) => /^No[A-Z]/.test(v.name));
    if (tag < 0) throw new Error(`${at}: no variant for Rust ${node.k} ${hostName}`);
    const variant = def.variants[tag];
    const at2 = `${at}.${variant.name}`;
    if (def.name === "MIROperation" && variant.name === "Binary" && node.k === "struct") {
      const dispatch = rustField(node, "dispatch");
      let trap = false;
      if (dispatch?.k === "unit" && dispatch.name === "Primitive") trap = false;
      else if (dispatch?.k === "struct" && dispatch.name === "Prelude") {
        if (this.trapRoutes === null) throw new Error(`${at2}: overflow derives from the whole program's checked Prelude routes`);
        trap = this.trapRoutes.has(rustId(rustField(dispatch, "call")));
      } else throw new Error(`${at2}: unsupported checked binary dispatch`);
      node = { ...node, fields: node.fields.filter(([name]) => name !== "overflow").concat([["overflow", { k: "unit", name: trap ? "Trap" : "Unchecked" }]]) };
    }
    // Native MIR preformats values before BuildString. Its Value(v) therefore
    // means Display, exactly as RuntimeMirCodec.rs's string-part conversion.
    if (def.name === "MIRStringPartKind" && node.k === "tuple" && node.name === "Value") {
      if (node.items.length !== 1) throw new Error(`${at2}: native string part must carry one value`);
      node = { ...node, items: [...node.items, { k: "unit", name: "Display" }] };
    }
    let values = [];
    if (node.k === "struct") {
      const renames = def.name === "MIRImportKind" && variant.name === "Unqualified" ? { module: "module_id" } : {};
      const host = new Map(node.fields.map(([field, value]) => [fold(renames[field] ?? field), value]));
      // By checked name where shared; otherwise exact positional correspondence.
      const named = variant.fields.some((f) => host.has(fold(f.name)));
      if (!named && node.fields.length !== variant.fields.length) throw new Error(`${def.name}.${variant.name}: ${node.fields.length} Rust fields for ${variant.fields.length} checked fields`);
      values = named
        ? variant.fields.map((f) => {
            if (host.has(fold(f.name))) return this.convert(host.get(fold(f.name)), f.type, `${at2}.${f.name}`);
            throw new Error(`${def.name}.${variant.name}.${f.name}: missing checked Rust field; regenerate the MIR dump with the current compiler`);
          })
        : variant.fields.map((f, index) => this.convert(node.fields[index][1], f.type, `${at2}.${f.name}`));
    } else if (node.k === "tuple") {
      if (variant.fields.length === 1 && node.items.length > 1) values = [this.convert({ k: "tuple", name: null, items: node.items }, variant.fields[0].type, at2)];
      else if (variant.fields.length > 1 && node.items.length === 1 && node.items[0].k === "struct") {
        // `Variant(Payload { a, b })` against a Jet variant `Variant(a, b)`.
        return this.enumValue({ k: "struct", name: hostName, fields: node.items[0].fields }, def, where);
      } else {
        if (node.items.length !== variant.fields.length) throw new Error(`${def.name}.${variant.name}: ${node.items.length} Rust values for ${variant.fields.length} checked fields`);
        values = variant.fields.map((f, index) => this.convert(node.items[index], f.type, `${at2}.${f.name}`));
      }
    } else {
      if (variant.fields.length > 0) throw new Error(`${def.name}.${variant.name}: unit Rust variant cannot supply checked fields`);
      values = [];
    }
    return { tag, fields: values };
  }
}

function floatBits(raw) {
  let value;
  if (raw === "NaN") value = NaN;
  else if (raw === "inf") value = Infinity;
  else if (raw === "-inf") value = -Infinity;
  else value = Number(raw);
  const view = new DataView(new ArrayBuffer(8));
  view.setFloat64(0, value);
  return view.getBigInt64(0).toString();
}

// ---------------------------------------------------------------------------
// Line stream.

export function encode(value, type, schema, out) {
  if (type.k === "opt") {
    if (value.opt === null) out.push("0");
    else {
      out.push("1");
      encode(value.opt, type.of, schema, out);
    }
    return;
  }
  if (type.k === "list") {
    out.push(String(value.length));
    for (const item of value) encode(item, type.of, schema, out);
    return;
  }
  if (type.k === "map") {
    out.push(String(value.map.length));
    for (const [key, item] of value.map) {
      encode(key, type.key, schema, out);
      encode(item, type.value, schema, out);
    }
    return;
  }
  const name = type.name;
  if (name === "String") {
    out.push(/^[^\n\r\\]*$/.test(value) ? `s${value}` : `x${Buffer.from(value, "utf8").toString("hex")}`);
    return;
  }
  if (name === "Bool") {
    out.push(value ? "1" : "0");
    return;
  }
  if (PRIMITIVES.has(name)) {
    out.push(value.int);
    return;
  }
  const def = schema.get(name);
  if (def.kind === "struct") {
    def.fields.forEach((f, index) => encode(value.fields[index], f.type, schema, out));
    return;
  }
  out.push(String(value.tag));
  def.variants[value.tag].fields.forEach((f, index) => encode(value.fields[index], f.type, schema, out));
}

// ---------------------------------------------------------------------------
// Jet decoder for the line stream, generated from the schema.

// Decoder function names are snake_case (D-SHAPE-CASE1): `MIRTypeKind` ->
// `mir_type_kind`.
export function snake(name) {
  return name.replace(/([A-Z]+)([A-Z][a-z])/g, "$1_$2").replace(/([a-z0-9])([A-Z])/g, "$1_$2").toLowerCase();
}

function mangle(type) {
  if (type.k === "opt") return `opt_${mangle(type.of)}`;
  if (type.k === "list") return `list_${mangle(type.of)}`;
  if (type.k === "map") return `map_${mangle(type.key)}_${mangle(type.value)}`;
  return snake(type.name);
}

function spell(type) {
  if (type.k === "opt") return `${spell(type.of)}?`;
  if (type.k === "list") return `[${spell(type.of)}]`;
  if (type.k === "map") return `[${spell(type.key)}:${spell(type.value)}]`;
  return type.name;
}

export function jetDecoder(schema) {
  const lines = [];
  const helpers = new Map();
  const call = (type) => {
    if (type.k === "named") {
      if (type.name === "Int") return "mird_int(&r)";
      if (type.name === "String") return "mird_string(&r)";
      if (type.name === "Bool") return "mird_bool(&r)";
      if (type.name === "Float") return "mird_float(&r)";
      if (type.name === "U64") return "mird_u64(&r)";
      if (PRIMITIVES.has(type.name)) return `${type.name}{mird_int(&r)}`;
      return `mird_${snake(type.name)}(&r)`;
    }
    const name = `mird_${mangle(type)}`;
    if (!helpers.has(name)) {
      helpers.set(name, null);
      let body;
      if (type.k === "opt") {
        body = [`fn ${name}(r: &MIRDReader) -> ${spell(type)} {`, `    if mird_int(&r) == 0 -> return None`, `    Val(${call(type.of)})`, `}`];
      } else if (type.k === "list") {
        body = [`fn ${name}(r: &MIRDReader) -> ${spell(type)} {`, `    count :: mird_int(&r)`, `    out := ${spell(type)}{}`, `    loop _ in 0..<count -> &out.push(${call(type.of)})`, `    out`, `}`];
      } else {
        body = [
          `fn ${name}(r: &MIRDReader) -> ${spell(type)} {`,
          `    count :: mird_int(&r)`,
          `    out := ${spell(type)}{}`,
          `    loop _ in 0..<count {`,
          `        key :: ${call(type.key)}`,
          `        _ := &out.add(key, ${call(type.value)})`,
          `    }`,
          `    out`,
          `}`,
        ];
      }
      helpers.set(name, body.join("\n"));
    }
    return `${name}(&r)`;
  };
  for (const def of schema.values()) {
    if (def.kind === "struct") {
      const body = [`fn mird_${snake(def.name)}(r: &MIRDReader) -> ${def.name} {`];
      def.fields.forEach((f, index) => body.push(`    v${index} :: ${call(f.type)}`));
      body.push(`    ${def.name}{${def.fields.map((f, index) => `${f.name}: v${index}`).join(", ")}}`);
      body.push("}");
      lines.push(body.join("\n"));
    } else {
      const body = [`fn mird_${snake(def.name)}(r: &MIRDReader) -> ${def.name} {`, `    which :: mird_int(&r)`];
      def.variants.forEach((variant, index) => {
        if (variant.fields.length === 0) {
          body.push(`    if which == ${index} -> return ${def.name}.${variant.name}`);
        } else {
          body.push(`    if which == ${index} {`);
          variant.fields.forEach((f, position) => body.push(`        v${position} :: ${call(f.type)}`));
          body.push(`        return ${def.name}.${variant.name}{${variant.fields.map((f, position) => `${f.name}: v${position}`).join(", ")}}`);
          body.push("    }");
        }
      });
      body.push(`    panic("MIR stream: ${def.name} has no variant {which}")`);
      body.push("}");
      lines.push(body.join("\n"));
    }
  }
  return [...helpers.values(), ...lines].join("\n\n") + "\n";
}
