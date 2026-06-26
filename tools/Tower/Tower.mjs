#!/usr/bin/env node
// Tower — air-traffic control for the Jet project.
//
//   node Tower.mjs serve [--open] [--port N]   start the dashboard
//   node Tower.mjs status                       text snapshot of the board
//
// Std-only, zero dependencies. Reads/writes only tools/Tower/tower.json.
const args = process.argv.slice(2);
const cmd = args[0] || 'serve';
const flag = (n) => args.includes(`--${n}`);
const opt = (n, d) => { const i = args.indexOf(`--${n}`); return i >= 0 ? args[i + 1] : d; };

if (cmd === 'serve') {
  const { serve } = await import('./app/server.mjs');
  serve(Number(opt('port', 7878)), flag('open'));
} else if (cmd === 'status') {
  const { status } = await import('./app/cli.mjs');
  status();
} else {
  console.log('usage: Tower.mjs [serve|status]');
  process.exit(1);
}
