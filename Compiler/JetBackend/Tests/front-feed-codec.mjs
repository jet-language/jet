// Field-complete observer of Jet-produced MIR. Same schema/token format as
// mir-debug.mjs; generated writers are diagnostic-only, not the backend feed.
import { snake } from './mir-debug.mjs';

const primitives = new Set(['Int', 'U64', 'U8', 'U16', 'U32', 'I8', 'I16', 'I32', 'I64', 'Float', 'Bool', 'String']);
const spell = t => t.k === 'opt' ? `${spell(t.of)}?` : t.k === 'list' ? `[${spell(t.of)}]` : t.k === 'map' ? `[${spell(t.key)}:${spell(t.value)}]` : t.name;
const name = t => t.k === 'opt' ? `opt_${name(t.of)}` : t.k === 'list' ? `list_${name(t.of)}` : t.k === 'map' ? `map_${name(t.key)}_${name(t.value)}` : snake(t.name);
export function jetWriter(schema) {
  const helpers = new Map();
  const call = (t, v) => {
    const n = `mirw_${name(t)}`;
    if (!helpers.has(n)) {
      helpers.set(n, '');
      let body;
      if (t.k === 'opt') body = `    if v == {\n        .None -> &out.push("0")\n        .Val(item) -> {\n            &out.push("1")\n            ${call(t.of, 'item')}\n        }\n    }`;
      else if (t.k === 'list') body = `    &out.push("{v.len()}")\n    loop item in v -> ${call(t.of, 'item')}`;
      else if (t.k === 'map') body = `    &out.push("{v.len()}")\n    loop (key, item) in v {\n        ${call(t.key, 'key')}\n        ${call(t.value, 'item')}\n    }`;
      else if (t.name === 'String') body = '    text := "x"\n    digits :: "0123456789abcdef"\n    loop byte in v.bytes() {\n        code :: Int.from_u8(byte)\n        text = "{text}{digits.slice((code // 16)..<(code // 16 + 1))}{digits.slice((code % 16)..<(code % 16 + 1))}"\n    }\n    &out.push(text)';
      else if (t.name === 'Bool') body = '    &out.push(if v -> "1" else -> "0")';
      else if (t.name === 'Float') body = '    &out.push("{math.to_bits(v)}")';
      else if (primitives.has(t.name)) body = '    &out.push("{v}")';
      else {
        const def = schema.get(t.name);
        if (!def) throw new Error(`missing MIR schema type ${t.name}`);
        if (def.kind === 'struct') body = def.fields.map(f => `    ${call(f.type, `v.${f.name}`)}`).join('\n');
        else body = '    if v == {\n' + def.variants.map((variant, tag) => {
          const vars = variant.fields.map((_, i) => `v${i}`);
          return `        .${variant.name}${vars.length ? `(${vars.join(', ')})` : ''} -> {\n            &out.push("${tag}")\n${variant.fields.map((f, i) => `            ${call(f.type, vars[i])}\n`).join('')}        }`;
        }).join('\n') + '\n    }';
      }
      helpers.set(n, `fn ${n}(v: ${spell(t)}, out: &[String]) {\n${body}\n}`);
    }
    return `${n}(${v}, &out)`;
  };
  call({ k: 'named', name: 'MIRProgram' }, 'v');
  return [...helpers.values()].join('\n\n') + '\n';
}

// Decode into named JSON fields (rather than positional converter objects),
// retaining integer/float-bit text exactly. Reject truncated/trailing streams.
export function decodeStream(text, schema) {
  const lines = text.trimEnd().split('\n');
  let at = 0;
  const next = () => { if (at >= lines.length) throw new Error(`MIR stream truncated at ${at}`); return lines[at++]; };
  const count = () => { const raw = next(); if (!/^\d+$/.test(raw)) throw new Error(`bad MIR count ${raw}`); const n = Number(raw); if (!Number.isSafeInteger(n) || n > lines.length) throw new Error(`bad MIR count ${raw}`); return n; };
  const read = t => {
    if (t.k === 'opt') { const tag = next(); if (tag === '0') return null; if (tag !== '1') throw new Error(`bad option tag ${tag}`); return read(t.of); }
    if (t.k === 'list') return Array.from({ length: count() }, () => read(t.of));
    if (t.k === 'map') return Array.from({ length: count() }, () => [read(t.key), read(t.value)]);
    if (t.name === 'String') { const raw = next(); if (raw[0] === 's') return raw.slice(1); if (raw[0] !== 'x' || !/^(?:[0-9a-f]{2})*$/.test(raw.slice(1))) throw new Error('bad MIR string'); return Buffer.from(raw.slice(1), 'hex').toString('utf8'); }
    if (t.name === 'Bool') { const raw = next(); if (raw !== '0' && raw !== '1') throw new Error('bad MIR bool'); return raw === '1'; }
    if (primitives.has(t.name)) return next();
    const d = schema.get(t.name);
    if (d.kind === 'struct') return Object.fromEntries(d.fields.map(f => [f.name, read(f.type)]));
    const tag = count(); const variant = d.variants[tag]; if (!variant) throw new Error(`bad ${t.name} tag ${tag}`);
    return { variant: variant.name, ...Object.fromEntries(variant.fields.map(f => [f.name, read(f.type)])) };
  };
  const entry = next();
  const program = read({ k: 'named', name: 'MIRProgram' });
  if (at !== lines.length) throw new Error(`MIR stream has ${lines.length - at} trailing tokens`);
  return { entry, program };
}
