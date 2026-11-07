// Decode committed generated literals for exact process-edge report goldens.
// No runtime executes to manufacture the expected report.
import {readFileSync} from 'node:fs';

const traps = readFileSync(new URL('../Source/TrapMessages.jet', import.meta.url), 'utf8');
const scheduler = readFileSync(new URL('../Source/SchedulerMessages.jet', import.meta.url), 'utf8');

// The bytes of an `rt_static_text("...")` body, undoing the generators'
// escapes (Tools/agent/RuntimeGenerators.jet static_text).
const STATIC = /rt_static_text\("((?:[^"\\]|\\.)*)"\)/;
function staticBytes(body) {
    return Buffer.from(body.replace(/\\(.)|\{\{|\}\}/g, (match, escape) => {
        if (escape === undefined) return match[0];
        const plain = {n: '\n', '\\': '\\', '"': '"'}[escape];
        if (plain === undefined) throw Error(`unknown static text escape \\${escape}`);
        return plain;
    }), 'utf8');
}

// A literal is either one static text (`<name>_text`) or little-endian
// signed I64 word rows.
export function trapLiteral(name) {
    const text = traps.match(new RegExp(`^${name}_text: fn\\(\\) -> I64 \\{ ${STATIC.source} \\}$`, 'm'))?.[1];
    if (text !== undefined) return staticBytes(text);
    const body = traps.match(new RegExp(`^${name}: fn\\(\\) \\{\\n([\\s\\S]*?)^\\}`, 'm'))?.[1];
    if (!body) throw Error(`missing frozen trap literal ${name}`);
    const bytes = [];
    for (const row of body.matchAll(/trap_write_word\(I64\{(-?\d+)\}, I64\{(\d+)\}\)/g)) {
        const word = BigInt(row[1]);
        for (let i = 0; i < Number(row[2]); ++i) bytes.push(Number((word >> BigInt(i * 8)) & 255n));
    }
    return Buffer.from(bytes);
}

export function schedulerLiteral(name) {
    const body = scheduler.match(new RegExp(`^${name}: fn\\(\\) -> I64 \\{\\n([\\s\\S]*?)^\\}`, 'm'))?.[1];
    const length = body?.match(/scheduler_text_new\(I64\{(\d+)\}\)/)?.[1];
    const text = body?.match(STATIC)?.[1];
    if (length === undefined || text === undefined) throw Error(`missing frozen scheduler literal ${name}`);
    const bytes = staticBytes(text);
    if (bytes.length !== Number(length)) throw Error(`scheduler literal ${name} is ${bytes.length} bytes, not ${length}`);
    return bytes;
}
