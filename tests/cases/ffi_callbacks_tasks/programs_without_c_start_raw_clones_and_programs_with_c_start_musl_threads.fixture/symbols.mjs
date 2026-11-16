// The link shape of a built program, from its ELF symbol table:
// how many pthread functions it holds, whether musl's startup and the link's
// C thread functions are in it, and its startup.
import { readFileSync } from "node:fs";

const bytes = readFileSync(process.argv[2]);
const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
const sections = Number(view.getBigUint64(0x28, true));
const size = view.getUint16(0x3a, true);
const count = view.getUint16(0x3c, true);
const header = (k) => sections + k * size;
const names = new Set();
for (let k = 0; k < count; k++) {
    if (view.getUint32(header(k) + 4, true) !== 2) continue;
    const offset = Number(view.getBigUint64(header(k) + 0x18, true));
    const length = Number(view.getBigUint64(header(k) + 0x20, true));
    const strings = Number(view.getBigUint64(header(view.getUint32(header(k) + 0x28, true)) + 0x18, true));
    for (let at = offset; at < offset + length; at += 24) {
        const start = strings + view.getUint32(at, true);
        const end = bytes.indexOf(0, start);
        names.add(bytes.toString("latin1", start, end));
    }
}
if (names.size === 0) {
    console.log("no symbol table");
    process.exit(1);
}
const pthread = [...names].filter((name) => name.startsWith("pthread_")).length;
console.log(`pthread functions: ${pthread > 0}`);
console.log(`musl startup: ${names.has("__libc_start_main")}`);
console.log(`Jet startup: ${names.has("__jet_start")}`);
console.log(`C thread functions handed to the runtime: ${names.has("__jet_thread_create")}`);
