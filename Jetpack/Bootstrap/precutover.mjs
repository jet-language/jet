#!/usr/bin/env node
// TRANSITIONAL: delete once the compiler and syntax probe accept D-TYPE-SUFFIX1
// (`T?`, `-> T E!`) and D-CAP-RECEIVER1 (`&buf.append(..)`, `^buf.seal()`).
// Rewrites an assembled unit to the pre-cutover spellings so the old grammar
// probe can check everything else. Every rewrite keeps byte length and line
// breaks, so the probe's byte spans still map through jetpack.map.json.
//   precutover.mjs <in.jet> <out.jet>
import { readFileSync, writeFileSync } from "node:fs";

const [input, output] = process.argv.slice(2);

function rewriteLine(line) {
  // Receiver marks: `&name.method(` / `^name.method(` -> ` name.method(` (statement or arm start).
  line = line.replace(/(^\s*|->\s*|\{\s*)[&^](?=[a-z_]\w*(?:\[[^\]]*\])?(?:\.[a-z_]\w*)*\.[a-z_]\w*\()/g, "$1 ");
  // Failure contracts: `Name!` / `alias.Name!` before `{`, `-[`, `=`, or end of line -> `!Name`.
  line = line.replace(/(?<![\w.])((?:[a-z_]\w*\.)*[A-Z][A-Za-z0-9_]*)!(?=\s*(\{|-\[|=|$))/g, "!$1");
  // Optional suffix: `T?` -> `?T` (moves the mark to the start of the type term).
  let out = "";
  for (let i = 0; i < line.length; i += 1) {
    const ch = line[i];
    const next = line[i + 1] ?? "\n";
    const prev = out[out.length - 1] ?? "";
    // An optional typed head `T?{..}` is canonical but blocked by parser defect
    // #3686 in both spellings; probe it as the plain head `T {..}`.
    if (ch === "?" && next === "{" && /[\w\]>]/.test(prev)) {
      out += " ";
      continue;
    }
    if (ch === "?" && /[\w\])>]/.test(prev) && /[\s,)}{>=\]\n]/.test(next) && line[i - 1] !== "?") {
      let start = out.length - 1;
      if (prev === "]" || prev === ")" || prev === ">") {
        const open = prev === "]" ? "[" : prev === ")" ? "(" : "<";
        let depth = 0;
        for (; start >= 0; start -= 1) {
          if (out[start] === prev) depth += 1;
          else if (out[start] === open) {
            depth -= 1;
            if (depth === 0) break;
          }
        }
        if (prev === ">") while (start > 0 && /[\w.]/.test(out[start - 1])) start -= 1;
      } else {
        while (start > 0 && /[\w.]/.test(out[start - 1])) start -= 1;
      }
      out = `${out.slice(0, start)}?${out.slice(start)}`;
      continue;
    }
    out += ch;
  }
  return out;
}

const text = readFileSync(input, "utf8");
const rewritten = text.split("\n").map(rewriteLine).join("\n");
if (Buffer.byteLength(rewritten) !== Buffer.byteLength(text)) {
  console.error("precutover.mjs: rewrite changed the unit's byte length");
  process.exit(70);
}
writeFileSync(output, rewritten);
