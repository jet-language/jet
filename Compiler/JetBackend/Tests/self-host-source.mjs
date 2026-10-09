// Lexical helpers for static self-host inventories; not a replacement for MIR.
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
  for (const match of code.matchAll(/\bfn\s+([A-Za-z_]\w*(?:\.[A-Za-z_]\w*)?)\s*(?:<[^;\n()]*>)?\s*\(/g)) {
    let cursor = match.index + match[0].length, depth = 1;
    while (cursor < code.length && depth) {
      if (code[cursor] === "(") depth++;
      else if (code[cursor] === ")") depth--;
      cursor++;
    }
    const open = code.indexOf("{", cursor);
    const nextFn = code.indexOf("\nfn ", cursor);
    if (open < 0 || (nextFn >= 0 && nextFn < open)) continue;
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
