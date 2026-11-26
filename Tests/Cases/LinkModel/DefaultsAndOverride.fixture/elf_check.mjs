import fs from 'node:fs';

const buf = fs.readFileSync(process.argv[2]);
if (buf.length < 64 || buf[0] !== 0x7f || buf[1] !== 0x45 || buf[2] !== 0x4c || buf[3] !== 0x46) {
  console.error('not an ELF file');
  process.exit(2);
}
const phoff = Number(buf.readBigUInt64LE(32));
const phentsize = buf.readUInt16LE(54);
const phnum = buf.readUInt16LE(56);
const loads = [];
let interp = 'none';
let dynamic = false;
let dynOff = 0;
let dynSize = 0;
for (let i = 0; i < phnum; i++) {
  const at = phoff + i * phentsize;
  const type = buf.readUInt32LE(at);
  const offset = Number(buf.readBigUInt64LE(at + 8));
  const vaddr = Number(buf.readBigUInt64LE(at + 16));
  const filesz = Number(buf.readBigUInt64LE(at + 32));
  if (type === 1) loads.push({ offset, vaddr, filesz });
  if (type === 3) {
    let end = offset;
    while (end < offset + filesz && buf[end] !== 0) end++;
    interp = buf.subarray(offset, end).toString('utf8');
  }
  if (type === 2) {
    dynamic = true;
    dynOff = offset;
    dynSize = filesz;
  }
}
const needed = [];
if (dynamic) {
  let strtab = -1;
  const offsets = [];
  for (let at = dynOff; at + 16 <= dynOff + dynSize; at += 16) {
    const tag = Number(buf.readBigInt64LE(at));
    const val = Number(buf.readBigUInt64LE(at + 8));
    if (tag === 0) break;
    if (tag === 5) strtab = val;
    if (tag === 1) offsets.push(val);
  }
  const seg = loads.find((s) => strtab >= s.vaddr && strtab < s.vaddr + s.filesz);
  if (seg) {
    const table = seg.offset + (strtab - seg.vaddr);
    for (const off of offsets) {
      let end = table + off;
      while (end < buf.length && buf[end] !== 0) end++;
      needed.push(buf.subarray(table + off, end).toString('utf8'));
    }
  }
}
console.log(`dynamic: ${dynamic}`);
console.log(`interpreter: ${interp}`);
console.log(`needed: ${needed.length > 0 ? needed.join(',') : 'none'}`);
