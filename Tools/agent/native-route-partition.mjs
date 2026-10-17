// A source proof is provenance, never a claim that a legacy Rust carrier has
// the source declaration's layout. Unproven members remain seed-only.
const escape = (value) => value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
function block(source, start) {
  let depth = 0, quoted = false;
  for (let index = start; index < source.length; index++) {
    if (!quoted && source.startsWith("//", index)) {
      index = source.indexOf("\n", index);
      if (index < 0) return null;
      continue;
    }
    if (source[index] === '"' && source[index - 1] !== "\\") quoted = !quoted;
    if (quoted) continue;
    if (source[index] === "{") depth++;
    if (source[index] === "}" && --depth === 0) return source.slice(start + 1, index);
  }
  return null;
}
function hasFunctionBody(source, name) {
  const declaration = new RegExp(`\\bfn\\s+${escape(name)}\\s*\\(`, "g");
  for (const match of source.matchAll(declaration)) {
    const line = source.slice(source.lastIndexOf("\n", match.index) + 1, match.index);
    if (line.trimStart().startsWith("//")) continue;
    let end = match.index + match[0].length, depth = 1;
    for (; end < source.length && depth; end++) {
      if (source[end] === "(") depth++;
      if (source[end] === ")") depth--;
    }
    if (depth) continue;
    const open = source.indexOf("{", end);
    if (open < 0 || /[;}]|\bfn\b/.test(source.slice(end, open))) continue;
    if (block(source, open) !== null) return true;
  }
  return false;
}
export function sourceOwnedAdapterProofs(adapters, ledger, tirRoutes, readSource) {
  const modules = [...ledger.matchAll(/CoreSourceModule \{ module: "([^"]+)", alias: "[^"]+", path: "([^"]+)", owned_members: &\[([^\]]*)\]/g)]
    .map((row) => ({ module: row[1], path: row[2], members: new Set([...row[3].matchAll(/"([^"]+)"/g)].map((match) => match[1])) }));
  const types = new Map();
  for (const row of ledger.matchAll(/CoreSourceOwnedType \{ path: "([^"]+)", name: "([^"]+)" \}/g)) {
    const paths = types.get(row[2]) ?? [];
    paths.push(row[1]); types.set(row[2], paths);
  }
  const sources = new Map();
  const sourceAt = (path) => {
    if (!sources.has(path)) sources.set(path, readSource(path));
    return sources.get(path);
  };
  const method = (type, member) => {
    for (const path of types.get(type) ?? []) {
      const source = sourceAt(path);
      for (const declaration of source.matchAll(/\b(?:struct|enum|impl)\s+(\w+)(?:<[^\n{]*>)?\s*\{/g)) {
        if (declaration[1] !== type) continue;
        const body = block(source, declaration.index + declaration[0].length - 1);
        if (body && hasFunctionBody(body, member)) return { path, type, member };
      }
    }
    return null;
  };
  const arms = [...tirRoutes.matchAll(/\(((?:"[^"]+"\s*\|?\s*)+),\s*"([^"]+)"\)\s*=>\s*(?:\{\s*)?\("([^"]+)"/g)];
  const proofs = new Map();
  for (const adapter of adapters) {
    const labels = adapter.rows ?? [];
    if (!labels.length) continue;
    const functions = labels.map((label) => {
      for (const owner of modules) {
        if (!label.startsWith(owner.module + ".")) continue;
        const member = label.slice(owner.module.length + 1);
        if (owner.members.has(member) && hasFunctionBody(sourceAt(owner.path), member)) return { path: owner.path, module: owner.module, member };
      }
      return null;
    });
    if (functions.every(Boolean)) { proofs.set(adapter.name, { kind: "source-function", declarations: functions }); continue; }
    // Raw Core entrypoints mixed with a public wrapper still need a real C
    // provider; a method proof must not erase that second caller contract.
    if (labels.some((label) => label.startsWith("core.") && !label.startsWith("core.handle.") && !label.startsWith("core.math."))) continue;
    const candidates = [];
    for (const label of labels) {
      const math = /^core\.math\.(\w+)_([A-Za-z_]\w*)$/.exec(label);
      if (math) candidates.push([math[1], math[2]]);
      const handle = /^core\.handle\.(?:[^.]+\.)+([^.]+)$/.exec(label);
      if (handle) for (const parameter of adapter.params) if (parameter.handle) candidates.push([parameter.handle.name, handle[1]]);
    }
    for (const arm of arms) if (arm[3] === adapter.symbol) for (const type of [...arm[1].matchAll(/"([^"]+)"/g)].map((row) => row[1])) candidates.push([type, arm[2]]);
    if (adapter.symbol.includes("::")) candidates.push(adapter.symbol.split("::").slice(-2));
    for (const [type, member] of candidates) {
      const declaration = method(type, member);
      if (declaration) { proofs.set(adapter.name, { kind: "source-method", declarations: [declaration] }); break; }
    }
  }
  return proofs;
}

// The guard's b5a331841 invariant applies to retained native emissions. A
// missing provider never licenses the seed body in a Rust-free native pack.
export function nativeSeedOnlyExports(seedNames, adapters, providers) {
  return new Set([...seedNames, ...adapters.filter((row) => !row.external).map((row) => row.name)].filter((name) => !providers.has(name)));
}
export function excludeNativeSeedBodies(body, providers, seedOnly, validateProvider) {
  const parts = []; let copied = 0;
  for (const match of body.matchAll(/^    #\[no_mangle\]\n    pub extern "C" fn ([A-Za-z_]\w*)([^{}]*?)\{/gm)) {
    const [, name, signature] = match;
    if (!providers.has(name) && !seedOnly.has(name)) throw new Error(`unpartitioned Rust native-pack export ${name}`);
    if (providers.has(name)) validateProvider(name, signature);
    parts.push(body.slice(copied, match.index), "    #[cfg(not(jet_native_pack))]\n"); copied = match.index;
  }
  parts.push(body.slice(copied));
  return parts.join("");
}
