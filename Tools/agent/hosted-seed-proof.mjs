// The bootstrap seed is compiled by the Rust host, not Jet Host/Entry.
// Its real build result and diagnostics prove this boundary; only native
// stage1/stage2 require Jet's all-phases keep-going TSV.
export function hostedSeedCensus(buildRc, log) {
    const errors = [];
    for (const [index, line] of log.split('\n').entries()) {
        if (/^\s*(?:error(?:\s*\[|:)|internal compiler error\b|ICE\b|(?:thread\s+['"][^'"]+['"]\s+)?panic(?:ked)?\b)/i.test(line)) errors.push({line: index + 1, text: line});
    }
    if (buildRc !== 0 || errors.length) throw Error(`hosted seed failed: build rc=${buildRc}, error/ICE/panic rows=${errors.length}: ${JSON.stringify(errors)}`);
    return {
        schema: 'jet-hosted-seed-census/v1',
        boundary: 'Rust-hosted release compilation; not Jet Host/Entry all-phases execution',
        buildRc, errors,
        keepGoingTsvRequired: false,
    };
}

// This optional hosted MIR discovery is not Jet Host/Entry keep-going.
// Earlier summaries can be failed __comptime probes with a checked fallback;
// the final summary belongs to the complete compiler function sweep.
export function hostedLowerSweepCensus(log) {
    const summaries = [...log.matchAll(/^JET_DEBUG_LOWER_ALL summary: (\d+) of (\d+) functions failed to lower$/gm)]
        .map(row => ({failed: Number(row[1]), total: Number(row[2])}));
    const final = summaries.at(-1);
    if (!final || final.failed !== 0 || final.total < 1) {
        throw Error(`hosted MIR sweep is not clean: final=${JSON.stringify(final ?? null)}`);
    }
    return {
        schema: 'jet-hosted-lower-sweep-census/v1',
        failed: final.failed, total: final.total, summaries,
        boundary: 'Final hosted per-function MIR sweep only; build rc, emission and metadata remain separate gates',
    };
}
