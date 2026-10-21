// Shared post-closeout runner contract. This module never runs commands and has
// no CLI or qualification bypass. verify-full and native-smoke consume the same
// descriptors and pure result validator.
import { join } from 'node:path';

export const nativeSuiteNames = ['golden', 'ui', 'cases', 'registries'];

export function nativeSuiteCommands({ root, jet, head, scratch, evidence, node = process.execPath, suites = nativeSuiteNames }) {
    if (!suites.length || new Set(suites).size !== suites.length || suites.some(suite => !nativeSuiteNames.includes(suite))) throw Error('invalid or empty native suite selection');
    const definitions = [
        ['unsafe-ratchet', 'check-unsafe-ratchet.mjs', []],
        ['agent-doc-flags', 'check-agent-doc-flags.mjs', []],
        ['decision-index', 'decision-index.mjs', ['--check']],
        ['core-coverage', 'core-coverage-census.mjs', ['--check']],
    ].map(([scope, file, args]) => ({ scope, command: node, args: [join(root, 'Tools/agent', file), ...args], cwd: root, kind: 'gate', documents: {} }));
    for (const suite of suites) {
        if (suite !== 'registries') {
            // The fixture gate is `jet test Tools/testkit`; its `suite <name>` test reads JET (the
            // compiler under test) and JET_TEST_SCRATCH_DIR from the runner's environment and writes
            // the case verdicts to $JET_CI_EVIDENCE_DIR/<name>.json, the evidence report below.
            const report = join(evidence, `${suite}.json`);
            definitions.push({ scope: suite, command: jet, args: ['test', join(root, 'Tools/testkit'), `suite ${suite}`, '--serial', '--capture=all', '--color=never'], cwd: root, kind: 'testkit', documents: { report }, jet, suite });
        } else {
            const output = join(scratch, 'registries'), manifest = join(output, 'manifest.json'), report = join(evidence, 'registries.json');
            definitions.push({ scope: 'registry-manifest', command: node, args: [join(root, 'Tools/agent/compiler-registries.mjs'), 'manifest', head, '--out', output], cwd: root, kind: 'manifest', documents: { manifest }, head, root });
            definitions.push({ scope: 'registries', command: node, args: [join(root, 'Tools/agent/compiler-registries.mjs'), 'run', '--jet', jet, '--manifest', manifest, '--receipt', report], cwd: root, kind: 'registries', dependsOn: ['registry-manifest'], documents: { manifest, report }, jet, head, root });
        }
    }
    return definitions;
}

export function validateNativeSuiteResult(definition, child, documents = {}) {
    if (child.error || child.signal || child.status !== 0) return { status: 'FAIL', reason: child.error?.message ?? `exit ${child.status ?? '-'}, signal ${child.signal ?? '-'}` };
    try {
        if (definition.kind === 'testkit') {
            const report = documents.report;
            if (report?.jet !== definition.jet || !Array.isArray(report.results) || !report.results.length || report.results.some(row => row.status !== 'PASS' || row.suite !== definition.suite)) throw Error('missing, skipped, refused, wrong-suite or non-PASS testkit case verdict');
            return { status: 'PASS', total: report.results.length, passed: report.results.length };
        }
        if (definition.kind === 'manifest' || definition.kind === 'registries') {
            const manifest = documents.manifest;
            if (manifest?.head !== definition.head || manifest.repo !== definition.root || manifest.completeCompilerRegistries !== true || !Array.isArray(manifest.entries) || !manifest.entries.length || manifest.entries.some(row => row.status === 'BLOCKED')) throw Error('registry manifest is missing, unpinned, incomplete or BLOCKED');
            const ids = manifest.entries.map(row => row.id);
            if (new Set(ids).size !== ids.length) throw Error('duplicate registry manifest entries');
            if (definition.kind === 'registries') {
                const report = documents.report;
                if (report?.status !== 'PASS' || report.head !== definition.head || report.jet !== definition.jet || report.total !== ids.length || report.passed !== ids.length || report.blocked !== 0 || report.failed !== 0 || !Array.isArray(report.entries) || report.entries.length !== ids.length || report.entries.some(row => row.status !== 'PASS') || JSON.stringify(report.entries.map(row => row.id).sort()) !== JSON.stringify(ids.slice().sort())) throw Error('registry receipt has missing, unpinned, blocked, failed or non-PASS entries');
                return { status: 'PASS', total: report.total, passed: report.passed };
            }
            return { status: 'PASS', total: ids.length };
        }
        if (definition.kind !== 'gate') throw Error(`unknown native runner kind ${definition.kind}`);
        return { status: 'PASS' };
    } catch (error) {
        return { status: 'FAIL', reason: error.message };
    }
}
