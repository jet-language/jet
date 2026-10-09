// Decode committed generated literals for exact process-edge report goldens.
// No runtime executes to manufacture the expected report.
import {readFileSync} from 'node:fs';

const traps = readFileSync(new URL('../Source/TrapMessages.jet', import.meta.url), 'utf8');
const scheduler = readFileSync(new URL('../Source/SchedulerMessages.jet', import.meta.url), 'utf8');

export function trapLiteral(name) {
    const body = traps.match(new RegExp(`^fn ${name}\\(\\) \\{\\n([\\s\\S]*?)^\\}`, 'm'))?.[1];
    if (!body) throw Error(`missing frozen trap literal ${name}`);
    const bytes = [];
    for (const row of body.matchAll(/trap_write_word\(I64\{(-?\d+)\}, I64\{(\d+)\}\)/g)) {
        const word = BigInt(row[1]);
        for (let i = 0; i < Number(row[2]); ++i) bytes.push(Number((word >> BigInt(i * 8)) & 255n));
    }
    return Buffer.from(bytes);
}

export function schedulerLiteral(name) {
    const body = scheduler.match(new RegExp(`^fn ${name}\\(\\) -> I64 \\{\\n([\\s\\S]*?)^\\}`, 'm'))?.[1];
    const length = body?.match(/scheduler_text_new\(I64\{(\d+)\}\)/)?.[1];
    if (length === undefined) throw Error(`missing frozen scheduler literal ${name}`);
    const bytes = Buffer.alloc(Number(length));
    for (const row of body.matchAll(/sync_store\(data \+ I64\{(\d+)\}, I64\{(-?\d+)\}\)/g)) {
        const offset = Number(row[1]), word = BigInt(row[2]);
        for (let i = 0; i < Math.min(8, bytes.length - offset); ++i) bytes[offset + i] = Number((word >> BigInt(i * 8)) & 255n);
    }
    return bytes;
}
