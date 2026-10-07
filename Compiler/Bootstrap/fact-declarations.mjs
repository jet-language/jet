// Canonical Facts.jet declaration reader shared by registry projections.
// This is the same declaration grammar read by Registry.rs::fact_declaration.
export function factDeclarations(source) {
  const rows = [];
  for (const raw of source.split('\n')) {
    const line = raw.trim();
    if (!line || line.startsWith('//')) continue;
    const declaration = line.match(/^fact (\w+)\((.*)\)$/);
    if (!declaration) throw new Error(`invalid fact declaration: ${line}`);
    const fields = new Map();
    let start = 0, depth = 0, quoted = false, escaped = false;
    const body = declaration[2];
    const entries = [];
    for (let i = 0; i < body.length; i++) {
      const c = body[i];
      if (quoted) { if (escaped) escaped = false; else if (c === '\\') escaped = true; else if (c === '"') quoted = false; }
      else if (c === '"') quoted = true;
      else if ('[({'.includes(c)) depth++;
      else if ('])}'.includes(c)) depth--;
      else if (c === ',' && depth === 0) { entries.push(body.slice(start, i).trim()); start = i + 1; }
    }
    entries.push(body.slice(start).trim());
    for (const entry of entries) {
      const field = entry.match(/^\$(\w+):\s*(.*)$/);
      if (!field || fields.has(field[1])) throw new Error(`invalid/duplicate fact field: ${entry}`);
      fields.set(field[1], field[2]);
    }
    const required = key => { if (!fields.has(key)) throw new Error(`missing $${key}: ${line}`); return fields.get(key); };
    const text = value => JSON.parse(value);
    const target = required('holds').replace(/^\./, '');
    const safe = required('safe').replace(/^\./, '').toLowerCase();
    const gatesText = required('gates');
    if (!/^\[.*\]$/.test(gatesText) || !['Value', 'Scope', 'Build'].includes(target) || !['gain', 'shrink', 'discharge', 'none'].includes(safe)) throw new Error(`invalid fact law: ${line}`);
    const gates = gatesText.slice(1, -1).split(',').map(x => x.trim()).filter(Boolean).map(x => x.startsWith('"') ? text(x) : x.replace(/^\./, ''));
    const name = fields.has('name') ? text(fields.get('name')) : declaration[1];
    if (rows.some(row => row.name === name)) throw new Error(`duplicate fact name: ${name}`);
    rows.push({name, target, safe, gates, identity: fields.get('identity') === 'true', publishedBy: fields.get('proved_by') ?? null, decision: text(required('decision'))});
  }
  return rows;
}
