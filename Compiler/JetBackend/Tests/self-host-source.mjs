// Lexical helpers for static self-host inventories; not a replacement for the compiler IR.
export function codeOnly(text) {
  let result = "", at = 0, blockDepth = 0;
  while (at < text.length) {
    if (blockDepth) {
      if (text.startsWith("/*", at)) { blockDepth++; result += "  "; at += 2; }
      else if (text.startsWith("*/", at)) { blockDepth--; result += "  "; at += 2; }
      else { result += text[at] === "\n" ? "\n" : " "; at++; }
    } else if (text.startsWith("//", at)) {
      while (at < text.length && text[at] !== "\n") { result += " "; at++; }
    } else if (text.startsWith("/*", at)) { blockDepth = 1; result += "  "; at += 2; }
    else if (text[at] === '"') {
      const triple = text.startsWith('"""', at);
      const delimiter = triple ? '"""' : '"';
      result += " ".repeat(delimiter.length); at += delimiter.length;
      while (at < text.length && !text.startsWith(delimiter, at)) {
        if (!triple && text[at] === "\\") { result += "  "; at += 2; }
        else { result += text[at] === "\n" ? "\n" : " "; at++; }
      }
      if (at < text.length) { result += " ".repeat(delimiter.length); at += delimiter.length; }
    } else { result += text[at]; at++; }
  }
  return result;
}

export function functionsIn(text, path) {
  const code = codeOnly(text);
  const functions = [];
  // A declaration `name: fn(…) … {` or `Type.name: fn<T>(…) … {` starts its
  // line; a nested local function stays `fn name(…) {`. A head without a
  // body brace on its line is a field or a bodiless declaration.
  for (const match of code.matchAll(/^[ \t]*(?:(?:pub|priv)(?:\([^)\n]*\))?\s+)?(?:#[A-Za-z_]\w*(?:\([^)\n]*\))?\s+)*([A-Za-z_]\w*(?:\.[A-Za-z_]\w*)?):\s*fn\s*(?:<[^;\n()]*>)?\s*\(|\bfn\s+([A-Za-z_]\w*)\s*(?:<[^;\n()]*>)?\s*\(/gm)) {
    match[1] ??= match[2];
    let cursor = match.index + match[0].length, depth = 1;
    while (cursor < code.length && depth) {
      if (code[cursor] === "(") depth++;
      else if (code[cursor] === ")") depth--;
      cursor++;
    }
    const open = code.indexOf("{", cursor);
    const lineEnd = code.indexOf("\n", cursor);
    if (open < 0 || (lineEnd >= 0 && lineEnd < open)) continue;
    cursor = open + 1; depth = 1;
    while (cursor < code.length && depth) {
      if (code[cursor] === "{") depth++;
      else if (code[cursor] === "}") depth--;
      cursor++;
    }
    if (depth) throw new Error(`unclosed function ${path}:${match[1]}`);
    const line = text.slice(0, match.index).split("\n").length;
    functions.push({ name: match[1], path, line, body: code.slice(open + 1, cursor - 1), rawBody: text.slice(open + 1, cursor - 1), code, start: match.index, end: cursor });
  }
  return functions;
}
