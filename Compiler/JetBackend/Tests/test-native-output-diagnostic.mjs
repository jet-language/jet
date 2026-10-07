#!/usr/bin/env node
// Exercise the real registry, renderer and Host callback against its UI snapshot.
import assert from 'node:assert/strict';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
const repo=resolve(dirname(fileURLToPath(import.meta.url)),'../../..');
const [outArg,jetArg]=process.argv.slice(2);
if(!outArg||!jetArg) throw new Error('expected OUT CHECKER_JET');
const out=resolve(outArg); mkdirSync(out,{recursive:true});
const files=['Compiler/JetFoundation/Source/Diagnostics/Diagnostic.jet','Compiler/JetFoundation/Source/Registry/Diagnostics.jet','Compiler/JetFoundation/Source/Registry/DiagnosticRows.jet','Compiler/JetFoundation/Source/Diagnostics/Render.jet'];
const seen=new Set();
const parts=files.map(file=>readFileSync(join(repo,file),'utf8').replace(/^use core\.[^\n]*$/gm,line=>{if(seen.has(line))return '';seen.add(line);return line;}));
const host=readFileSync(join(repo,'Compiler/JetCli/Source/Cli/Host.jet'),'utf8');
const helper=host.match(/^fn jet_cli_native_test_unrun_check_report\(\)[\s\S]*?^}/m)?.[0];
assert.ok(helper,'Host must own the actual registered fatal output-check callback');
parts.push(helper,'use core.term as snapshot_term\nfn run() {\n    output :: snapshot_term.stdout()\n    &output.write(jet_cli_native_test_unrun_check_report()).drop("snapshot stdout")\n}\n');
writeFileSync(join(out,'snapshot.jet'),parts.join('\n\n'));
writeFileSync(join(out,'package.jet'),'name: "native_output_check_diagnostic"\nversion: "0.0.1"\n');
const run=spawnSync(resolve(jetArg),['run',join(out,'snapshot.jet')],{cwd:out,env:process.env,encoding:'utf8',maxBuffer:64<<20});
writeFileSync(join(out,'stdout'),run.stdout??'');writeFileSync(join(out,'stderr'),run.stderr??'');
assert.equal(run.status,0,run.stderr??String(run.signal));
assert.equal(run.stdout,readFileSync(join(repo,'tests/cli/native_output_check_fatal.txt'),'utf8'));
console.log('native output-check diagnostic UI snapshot: passed');
