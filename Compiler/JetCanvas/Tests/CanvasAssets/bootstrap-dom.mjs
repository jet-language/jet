// DEV-A1 bootstrap leaf scenarios. Consume only HTML emitted by the real Jet
// functions on stdin; execute its inline JS with Node's actual JS engine.
// This is NOT a DOM implementation or a browser/app/host integration claim.
// Full DOM controls, drafts, reconnect, source writes, and session authorities
// remain Tools/canvas-test integration scenarios after the host and A2 land.
import assert from 'node:assert/strict';
import { Script, createContext } from 'node:vm';

const scenarios = {
  default: { base: '/canvas', src: '/canvas/app.js?canvas_ui=blueprint23' },
  nested: { base: '/project/canvas/', src: '/project/canvas//app.js?canvas_ui=blueprint23' },
  empty: { base: '', src: '/app.js?canvas_ui=blueprint23' },
  opaque: { base: '/雪?q=1&path=../#leaf', src: '/雪?q=1&path=../#leaf/app.js?canvas_ui=blueprint23' },
  hostile: {
    base: '/a"\\\n\r\t<>&\'雪\u007f\u0085\u0000',
    src: '/a\\"\\\\\\n\\r\\t<>&\'雪\\u007f\\u0085\\u0000/app.js?canvas_ui=blueprint23',
  },
  marker: { base: '/__JET_CANVAS_BOOTSTRAP__', src: '/__JET_CANVAS_BOOTSTRAP__/app.js?canvas_ui=blueprint23' },
  query: {
    base: '', src: '/?jet_panel_app=1&canvas_ui=blueprint23',
    routes: {
      __JET_CANVAS_GRAPH__: '/?jet_panel_graph=1',
      __JET_CANVAS_TX__: '/canvas/transaction',
      __JET_CANVAS_QUERY__: '/canvas/query',
      __JET_CANVAS_SCM__: '/canvas/source-control',
      __JET_CANVAS_PROOF__: '/canvas/proof',
      __JET_CANVAS_COMMAND__: '/canvas/command',
    },
  },
};
const name = process.argv[2];
assert(Object.hasOwn(scenarios, name), `unknown bootstrap scenario ${name}`);
let html = '';
process.stdin.setEncoding('utf8');
for await (const chunk of process.stdin) html += chunk;

// Extract raw script elements, preserving raw-text contents and attribute
// payload bytes. In particular this does not silently HTML-normalize the
// historical JSON-escaped src attribute into a different transport contract.
const scripts = [...html.matchAll(/<script(?:\s+src="([\s\S]*?)")?>([\s\S]*?)<\/script>/g)];
assert.equal(scripts.length, 2, 'one bootstrap and one app entry must be emitted');

const writes = [];
const window = new Proxy(Object.create(null), {
  set(target, key, value) {
    writes.push(key);
    target[key] = value;
    return true;
  },
});
const context = createContext({ window });
const loaded = [];
for (const [index, [, src, inline]] of scripts.entries()) {
  if (src === undefined) {
    // Compiles AND executes the emitted source, rather than searching for its
    // assignments in the HTML. Syntax errors and injected statements fail.
    new Script(inline, { filename: `canvas-${name}-${index}.js` }).runInContext(context, { timeout: 1000 });
  } else {
    assert.equal(inline, '');
    assert.equal(window.__JET_CANVAS_BASE__, scenarios[name].base, 'base must be installed before app loading');
    loaded.push(src);
  }
}
const expected = { __JET_CANVAS_BASE__: scenarios[name].base, ...scenarios[name].routes };
assert.deepEqual({ ...window }, expected, 'only the intended route globals may be installed');
assert.deepEqual(writes, Object.keys(expected), 'bootstrap route assignment order');
assert.deepEqual(loaded, [scenarios[name].src], 'exact versioned app route payload');
console.log(`bootstrap ${name}: evaluated globals and ordered app route OK`);
