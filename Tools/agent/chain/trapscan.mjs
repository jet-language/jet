// trapscan.mjs ELF: list the functions of a built jet that carry a keep-going
// `todo` trap (#5662): `lea "todo"(%rip)` .. `mov $4,%rsi; call jet_rt_unreachable`.
// A sema keep-going stub lowers to exactly that; the compiler source has no
// `todo` expression, so a clean compiler prints nothing. Exit 1 when any is
// found, 2 when the binary has no symbol table or no jet_rt_unreachable.
import { readFileSync } from "node:fs";
const b = readFileSync(process.argv[2]);
const u64 = (o) => Number(b.readBigUInt64LE(o));
const shoff = u64(0x28), shentsize = b.readUInt16LE(0x3a), shnum = b.readUInt16LE(0x3c);
const phoff = u64(0x20), phentsize = b.readUInt16LE(0x36), phnum = b.readUInt16LE(0x38);
const loads = [];
for (let i = 0; i < phnum; i++) {
  const p = phoff + i * phentsize;
  if (b.readUInt32LE(p) !== 1) continue;
  loads.push({ x: (b.readUInt32LE(p + 4) & 1) !== 0, off: u64(p + 8), vaddr: u64(p + 16), filesz: u64(p + 32) });
}
const fileOf = (va) => {
  for (const l of loads) if (va >= l.vaddr && va < l.vaddr + l.filesz) return l.off + (va - l.vaddr);
  return -1;
};
const funcs = [];
let unreachable = -1;
for (let i = 0; i < shnum; i++) {
  const s = shoff + i * shentsize;
  if (b.readUInt32LE(s + 4) !== 2) continue; // SHT_SYMTAB
  const off = u64(s + 0x18), size = u64(s + 0x20), link = b.readUInt32LE(s + 0x28), ent = u64(s + 0x38);
  const str = u64(shoff + link * shentsize + 0x18);
  for (let at = off; at + ent <= off + size; at += ent) {
    if ((b[at + 4] & 0xf) !== 2) continue; // STT_FUNC
    const nameAt = str + b.readUInt32LE(at);
    const name = b.toString("latin1", nameAt, b.indexOf(0, nameAt));
    const value = u64(at + 8);
    if (name === "jet_rt_unreachable") unreachable = value;
    funcs.push([value, name]);
  }
}
if (unreachable < 0) {
  console.error("trapscan: no symbol table or no jet_rt_unreachable in " + process.argv[2]);
  process.exit(2);
}
funcs.sort((x, y) => x[0] - y[0]);
const owner = (va) => {
  let lo = 0, hi = funcs.length - 1, best = "?";
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (funcs[mid][0] <= va) { best = funcs[mid][1]; lo = mid + 1; } else hi = mid - 1;
  }
  return best;
};
const found = new Set();
for (const l of loads) {
  if (!l.x) continue;
  const end = l.off + l.filesz;
  for (let at = l.off; at + 12 <= end; at++) {
    // mov $0x4,%rsi ; call rel32
    if (b[at] !== 0x48 || b[at + 1] !== 0xc7 || b[at + 2] !== 0xc6 || b.readUInt32LE(at + 3) !== 4 || b[at + 7] !== 0xe8) continue;
    const call = l.vaddr + (at + 7 - l.off);
    if (call + 5 + b.readInt32LE(at + 8) !== unreachable) continue;
    for (let back = at - 7; back >= Math.max(l.off, at - 24); back--) {
      if (b[back] !== 0x48 || b[back + 1] !== 0x8d || b[back + 2] !== 0x05) continue;
      const target = fileOf(l.vaddr + (back - l.off) + 7 + b.readInt32LE(back + 3));
      if (target >= 0 && b.toString("latin1", target, target + 4) === "todo") found.add(owner(call));
      break;
    }
  }
}
for (const name of [...found].sort()) console.log(name);
process.exit(found.size > 0 ? 1 : 0);
