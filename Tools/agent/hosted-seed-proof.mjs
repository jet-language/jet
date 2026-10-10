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
