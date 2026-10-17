// Testkit's one-based I/N convention; validate the full manifest before calling.
export function partitionRegistries(entries, shard) {
    if (shard === undefined) return entries;
    const match = /^([1-9]\d*)\/([1-9]\d*)$/.exec(shard);
    if (!match) throw Error('invalid registry shard; expected I/N');
    const index = Number(match[1]), count = Number(match[2]);
    if (!Number.isSafeInteger(index) || !Number.isSafeInteger(count) || index > count) {
        throw Error('invalid registry shard I/N');
    }
    return [...entries].sort((a, b) => a.id < b.id ? -1 : a.id > b.id ? 1 : 0)
        .filter((_, offset) => offset % count === index - 1);
}
