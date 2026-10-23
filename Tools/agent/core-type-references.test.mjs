import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import test from 'node:test';
import {parseCoreSource} from './check-core-surface-ledger.mjs';

const source = readFileSync(new URL('../../Compiler/Prelude/Core.jet', import.meta.url), 'utf8');
function typeIn(facts, module, name) {
    return facts.modules.find(row => row.module === module).types.find(row => row.name === name);
}
test('catalog reexports share the defining enum variants and their ordinals', () => {
    const facts = parseCoreSource(source);
    for (const name of ['Aggregate', 'Backend', 'Channel', 'FacetKind', 'FilterOp', 'Interaction', 'LegendPosition', 'Mark', 'ScaleKind', 'Support']) {
        const owner = typeIn(facts, 'core.data.plot', 'JetDataPlot' + name);
        const reexport = typeIn(facts, 'core.data', 'JetDataPlot' + name);
        assert.strictEqual(reexport.variants, owner.variants);
        assert.equal(reexport.genericArity, owner.genericArity);
        assert.equal(reexport.kind, owner.kind);
    }
});
test('changing the defining variant list cannot leave a stale reexport', () => {
    const changed = source.replace('JetDataPlotAggregate:plain(None|Count|Sum|Mean|Min|Max)', 'JetDataPlotAggregate:plain(None|Sum|Count|Mean|Min|Max)');
    assert.notEqual(changed, source);
    const facts = parseCoreSource(changed);
    assert.deepEqual(typeIn(facts, 'core.data', 'JetDataPlotAggregate').variants, ['None', 'Sum', 'Count', 'Mean', 'Min', 'Max']);
});
test('unknown catalog type-reference targets fail loudly', () => {
    assert.throws(() => parseCoreSource(source.replace('JetDataPlotAggregate:from(core.data.plot)', 'JetDataPlotAggregate:from(core.missing)')), /unknown Core type reference `core\.missing\.JetDataPlotAggregate`/);
});
test('cyclic catalog type-reference targets fail loudly', () => {
    const cyclic = source.replace('JetDataPlotAggregate:plain(None|Count|Sum|Mean|Min|Max)', 'JetDataPlotAggregate:from(core.data)');
    assert.throws(() => parseCoreSource(cyclic), /cyclic Core type reference/);
});
test('private ExpiringValue ownership does not expand the public time types', () => {
    const facts = parseCoreSource(source);
    const owner = facts.sourceModules.find(row => row.module === 'core.time');
    assert.ok(owner.ownedTypes.includes('ExpiringValue'));
    assert.ok(!facts.modules.find(row => row.module === 'core.time').types.some(row => row.name === 'ExpiringValue'));
});
test('source ownership still requires the exact source declaration', () => {
    const changed = source.replace('TimeError, ExpiringValue }', 'TimeError, MissingPrivateType }');
    assert.notEqual(changed, source);
    assert.throws(() => parseCoreSource(changed), /does not declare owned type `core\.time\.MissingPrivateType`/);
});
