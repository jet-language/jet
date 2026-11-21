// How a web build would carry a call's destination: as one more i32
// parameter, which changes the function's wasm type, or in a mutable wasm
// global the caller sets before the call, which changes no type. The
// module is written here byte by byte, so the probe needs nothing but
// node. Each builder stores `items` Ints at a fixed address (the list a
// real builder fills) and reads the destination once, as returns.jet's
// `arg` row reads its word. A web build runs a program's Jet code as wasm
// (app.wasm); JavaScript only calls its exports, each an entry that would
// set the global to heap.
// Conventions: plain (no destination), param (an argument), global (the
// global, set before each call). Forms: indirect (call_indirect through
// the table, as a function value calls) and direct (a named call).
// Run: node convention.mjs [calls] [repeats]
const calls = Number(process.argv[2] ?? 20000000)
const repeats = Number(process.argv[3] ?? 7)

const uleb = (n) => {
    const out = []
    do {
        let byte = n & 0x7f
        n >>>= 7
        if (n !== 0) byte |= 0x80
        out.push(byte)
    } while (n !== 0)
    return out
}
const sleb = (n) => {
    const out = []
    for (;;) {
        const byte = n & 0x7f
        n >>= 7
        if ((n === 0 && (byte & 0x40) === 0) || (n === -1 && (byte & 0x40) !== 0)) {
            out.push(byte)
            return out
        }
        out.push(byte | 0x80)
    }
}
const vec = (items) => [...uleb(items.length), ...items.flat()]
const section = (id, body) => [id, ...uleb(body.length), ...body]
const name = (text) => vec([...Buffer.from(text)])

const I32 = 0x7f
const op = {
    block: [0x02, 0x40], loop: [0x03, 0x40], end: [0x0b],
    br: (d) => [0x0c, d], brIf: (d) => [0x0d, d],
    get: (l) => [0x20, l], set: (l) => [0x21, l],
    globalGet: [0x23, 0], globalSet: [0x24, 0],
    i32: (v) => [0x41, ...sleb(v)],
    add: [0x6a], shl: [0x74], geS: [0x4e], extend: [0xac],
    store64: [0x37, 3, 0],
    call: (f) => [0x10, f], callIndirect: (t) => [0x11, t, 0x00],
}

// A builder (i, items[, dest]) with one local k: stores i + dest at 64,
// then i + k at 64 + 8k for k in 1..<items, and returns items.
const builder = (mode) => {
    const k = mode === 'param' ? 3 : 2
    const dest = mode === 'param' ? op.get(2) : mode === 'global' ? op.globalGet : []
    const first = mode === 'plain' ? [] : [...dest, ...op.add]
    return [
        [1, 1, I32],
        op.i32(64), op.get(0), first, op.extend, op.store64,
        op.i32(1), op.set(k),
        op.block, op.loop,
        op.get(k), op.get(1), op.geS, op.brIf(1),
        op.get(k), op.i32(3), op.shl, op.i32(64), op.add,
        op.get(0), op.get(k), op.add, op.extend, op.store64,
        op.get(k), op.i32(1), op.add, op.set(k),
        op.br(0),
        op.end, op.end,
        op.get(1),
        op.end,
    ].flat()
}

// A bench (n, dest) -> sum of the results of n calls, with locals acc, i.
const bench = (mode, target, indirect) => [
    [1, 2, I32],
    op.block, op.loop,
    op.get(3), op.get(0), op.geS, op.brIf(1),
    mode === 'global' ? [...op.get(1), ...op.globalSet] : [],
    op.get(2),
    op.get(3), op.i32(2), mode === 'param' ? op.get(1) : [],
    indirect ? [...op.i32(target), ...op.callIndirect(mode === 'param' ? 1 : 0)] : op.call(target),
    op.add, op.set(2),
    op.get(3), op.i32(1), op.add, op.set(3),
    op.br(0),
    op.end, op.end,
    op.get(2),
    op.end,
].flat()

const modes = ['plain', 'param', 'global']
const body = (code) => [...uleb(code.length), ...code]
const functions = [
    ...modes.map((mode) => builder(mode)),
    ...modes.map((mode, at) => bench(mode, at, true)),
    ...modes.map((mode, at) => bench(mode, at, false)),
]
const bytes = [
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,
    ...section(1, vec([
        [0x60, ...vec([I32, I32]), ...vec([I32])],
        [0x60, ...vec([I32, I32, I32]), ...vec([I32])],
    ])),
    ...section(3, vec([[0], [1], [0], [0], [0], [0], [0], [0], [0]])),
    ...section(4, vec([[0x70, 0x00, 3]])),
    ...section(5, vec([[0x00, 1]])),
    ...section(6, vec([[I32, 0x01, ...op.i32(0), ...op.end]])),
    ...section(7, vec(modes.flatMap((mode, at) => [
        [...name(`indirect_${mode}`), 0x00, 3 + at],
        [...name(`direct_${mode}`), 0x00, 6 + at],
    ]))),
    ...section(9, vec([[0x00, ...op.i32(0), ...op.end, ...vec([[0], [1], [2]])]])),
    ...section(10, vec(functions.map(body))),
]
const { instance } = await WebAssembly.instantiate(new Uint8Array(bytes))

const rows = []
for (const mode of modes) {
    rows.push([`wasm indirect ${mode}`, () => instance.exports[`indirect_${mode}`](calls, 0)])
    rows.push([`wasm direct ${mode}`, () => instance.exports[`direct_${mode}`](calls, 0)])
}
const taken = new Map(rows.map(([label]) => [label, []]))
// Warm every row, then alternate them within each repeat so that a busy
// host's drift lands on all of them alike.
for (const [, run] of rows) run()
for (let repeat = 0; repeat < repeats; repeat++) {
    for (const [label, run] of rows) {
        const started = process.hrtime.bigint()
        const total = run()
        const ns = Number(process.hrtime.bigint() - started) / calls
        if (total !== 2 * calls) throw new Error(`${label}: total ${total}`)
        taken.get(label).push(ns)
    }
}
for (const [label, list] of taken) {
    const sorted = [...list].sort((a, b) => a - b)
    console.log(`${label}: ${calls} calls, min ${sorted[0].toFixed(3)} ns, median ${sorted[sorted.length >> 1].toFixed(3)} ns a call`)
}
