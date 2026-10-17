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

// Keep every rejected pass output visible, including checked comptime probes.
// Only rows after the last whole-function sweep belong to the main program.
// An accepted later pass cannot erase an earlier discarded main-program pass.
export function hostedOptimizationPassCensus(log) {
    const main = [...log.matchAll(/^JET_DEBUG_LOWER_ALL summary: (\d+) of (\d+) functions failed to lower$/gm)].at(-1);
    const summaries = [...log.matchAll(/^JET_DEBUG_LOWER_ALL optimization summary: operation=(\S+) pass=(\S+) violations=(\d+) output=(accepted|discarded)$/gm)]
        .map(row => ({operation: row[1], pass: row[2], violations: Number(row[3]), output: row[4], offset: row.index}));
    const violations = [...log.matchAll(/^JET_DEBUG_LOWER_ALL optimization: operation=(\S+) pass=(\S+) violation=(.*)$/gm)]
        .map(row => ({operation: row[1], pass: row[2], violation: row[3], offset: row.index}));
    const mainSummaries = summaries.filter(row => main && row.offset > main.index);
    const mainViolations = violations.filter(row => main && row.offset > main.index);
    return {
        schema: 'jet-hosted-optimization-pass-census/v1',
        summaries, violations, main_summaries: mainSummaries, main_violations: mainViolations,
        clean: mainSummaries.length > 0 && mainViolations.length === 0
            && mainSummaries.every(row => row.output === 'accepted' && row.violations === 0),
        boundary: 'Main-program verified optimization boundaries; every rejection stays a failure despite rollback and later accepted passes. Build rc, emission and metadata remain separate gates.',
    };
}

// Emission has its own item census; valid MIR is not proof of emitted Rust.
// Keep u64 identities as strings and preserve quoted diagnostics verbatim.
export function hostedEmissionCensus(log) {
    const main = [...log.matchAll(/^JET_DEBUG_LOWER_ALL summary: (\d+) of (\d+) functions failed to lower$/gm)].at(-1);
    const summaries = [...log.matchAll(/^JET_DEBUG_LOWER_ALL emission failures (\d+) of (\d+)$/gm)]
        .map(row => ({failed: Number(row[1]), total: Number(row[2]), offset: row.index}));
    const failures = [], malformed = [];
    const quoted = '"(?:[^"\\\\]|\\\\.)*"';
    const item = new RegExp(`^JET_DEBUG_LOWER_ALL emission: kind=(\\S+) id=(\\d+) name=(${quoted}) span=(\\d+)\\.\\.(\\d+) message=(${quoted})$`);
    const decode = value => {
        try { return JSON.parse(value); } catch { return null; }
    };
    for (const row of log.matchAll(/^JET_DEBUG_LOWER_ALL emission: (.*)$/gm)) {
        const fields = item.exec(row[0]);
        if (!fields) { malformed.push({raw: row[0], offset: row.index}); continue; }
        failures.push({
            kind: fields[1], id: fields[2],
            name: decode(fields[3]), name_debug: fields[3],
            span: {start: Number(fields[4]), end: Number(fields[5])},
            message: decode(fields[6]), message_debug: fields[6],
            raw: row[0], offset: row.index,
        });
    }
    const partials = [], malformedPartials = [];
    const partial = new RegExp(`^JET_DEBUG_LOWER_ALL emission partial Rust: path=(${quoted}) failures=(\\d+) items=(\\d+)$`);
    for (const row of log.matchAll(/^JET_DEBUG_LOWER_ALL emission partial Rust: (.*)$/gm)) {
        const fields = partial.exec(row[0]);
        if (!fields) { malformedPartials.push({raw: row[0], offset: row.index}); continue; }
        partials.push({path: decode(fields[1]), path_debug: fields[1],
            failed: Number(fields[2]), total: Number(fields[3]),
            admitted: false, role: 'diagnostic-partial', raw: row[0], offset: row.index});
    }
    const afterMain = row => main && row.offset > main.index;
    const mainSummaries = summaries.filter(afterMain);
    const mainFailures = failures.filter(afterMain);
    const mainMalformed = malformed.filter(afterMain);
    const final = mainSummaries.at(-1) ?? null;
    const mainPartials = partials.filter(afterMain);
    const mainMalformedPartials = malformedPartials.filter(afterMain);
    const lastPartial = mainPartials.at(-1) ?? null;
    const partialConsistent = lastPartial !== null && lastPartial.path !== null && final !== null
        && lastPartial.failed > 0 && lastPartial.failed === final.failed && lastPartial.total === final.total;
    return {
        schema: 'jet-hosted-emission-census/v1',
        summaries, failures, malformed,
        main_summaries: mainSummaries, main_failures: mainFailures, main_malformed_rows: mainMalformed,
        partials, malformed_partials: malformedPartials, main_partials: mainPartials,
        main_malformed_partials: mainMalformedPartials, latest_partial: lastPartial,
        partial_consistent_with_final: partialConsistent,
        final_summary: final,
        clean: final !== null && final.total > 0 && final.failed === 0
            && mainSummaries.every(row => row.failed === 0)
            && mainFailures.length === 0 && mainMalformed.length === 0
            && mainPartials.length === 0 && mainMalformedPartials.length === 0,
        boundary: 'Final hosted Rust-emitter item sweep only; successful emission, complete Rust output and rustc metadata remain separate gates.',
    };
}
