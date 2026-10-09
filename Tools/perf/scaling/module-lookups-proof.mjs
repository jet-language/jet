#!/usr/bin/env node
// Deterministic #4601 regression using an already-built Jet compiler. No build
// of the compiler or generated Rust backend is performed by this proof.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { options, run } from './harness.mjs';
const flags=Object.fromEntries(process.argv.slice(2).reduce((pairs,arg,i,args)=>i%2?pairs:[...pairs,[arg,args[i+1]]],[]));
if (!flags['--compiler'] || !flags['--root']) throw new Error('Usage: module-lookups-proof.mjs --compiler BUILT_JETC0 --root FRESH_DIRECTORY');
const root=path.resolve(flags['--root']);
assert(!fs.existsSync(root), `Proof directory already exists: ${root}`);
const compiler=path.resolve(flags['--compiler']);
const compilerSha256=createHash('sha256').update(fs.readFileSync(compiler)).digest('hex');
const sizes=[128,256,512,1024];
const metric='counter.sema.check.module_lookups';
const config=options(['--compiler',`candidate=bootstrap:${compiler}`,'--shape','comptime','--ladder',`comptime=${sizes.join(',')}`,'--mode','counters','--jobs','1','--layout','files','--seed','1','--memory-max','8G','--timeout','60','--work-dir',root,'--build-root',path.join(process.env.JET_SCRATCH_ROOT??'/mnt/jetscratch','scaling-module-lookups',path.basename(root))]);
const receipt=await run(config);
const series=receipt.series[0];
const counts=series?.points.map(point=>({n:point.n,ok:point.ok,lookups:point.metrics[metric]}))??[];
let error=null;
try {
  assert.equal(receipt.series.length,1,'Expected the one complete comptime/files series');
  assert(series.baseline?.ok,'The size-one baseline must compile successfully');
  const baseline=series.baseline.metrics[metric];
  assert(Number.isSafeInteger(baseline) && baseline>0,'Missing or disabled deterministic baseline counter');
  assert.deepEqual(counts.map(point=>point.n),sizes,'Every named rung is required');
  for (const point of counts) {
    assert(point.ok,`N=${point.n} did not compile successfully`);
    assert(Number.isSafeInteger(point.lookups) && point.lookups>baseline,`N=${point.n} has no valid growing deterministic counter`);
    // A fixed linear envelope anchored on the first successful rung, with
    // 25% packing slack; no n*log(n) or fitted exponent can hide quadratic work.
    assert(point.lookups-baseline<=(counts[0].lookups-baseline)*point.n/sizes[0]*1.25,
      `N=${point.n}: ${point.lookups-baseline} module lookups exceed the linear envelope`);
  }
} catch (failure) { error=failure.message; }
const result={metric,sizes,compiler,compilerSha256,counts,baseline:series?.baseline?.metrics?.[metric],passed:error===null,error,receipt:config.receipt,scope:'module-index regression only; unrelated full-gate failures remain in receipt.json'};
fs.writeFileSync(path.join(root,'module-lookups-proof.json'),JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify(result,null,2));
process.exitCode=error===null?0:1;
