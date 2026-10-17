import assert from 'node:assert/strict';import{execFileSync}from'node:child_process';import{readFileSync}from'node:fs';
const stdout=execFileSync(process.execPath,['showcase_harness.mjs'],{encoding:'utf8'}),lines=stdout.split(/\r?\n/);assert.equal(lines[0],'nodes=4');
for(const label of ['Altitude: 12,400 ft','Airspeed: 410 kt','Boosts: 0','Fuel: 40%'])assert.ok(lines.some(l=>l.includes('role=label aria='+JSON.stringify(label))));
const html=readFileSync('ui_showcase.html','utf8');assert.ok(html.includes('<button id="boost-btn" type="button" aria-label="Boost fuel" data-motion-state="idle">'));assert.ok(!html.includes('btn.disabled'));assert.equal(html.split('setTimeout(frame').length-1,2);assert.ok(html.includes('btn.dataset.motionState = "complete"'));
for(const[label,bg]of [['Altitude: 12,400 ft','#3366ff'],['Airspeed: 410 kt','#12b886'],['Boosts: 0','#8855ee'],['Fuel: 40%','#e8790c']])assert.ok(lines.some(l=>l.includes(JSON.stringify(label))&&l.includes('bg='+bg)));
assert.ok(lines.includes('boosts: Boosts: 2'));assert.ok(lines.includes('external-focus=true'));
// Independent original Rust expected_fuel_pct692-703; smoothstep oracle, never a captured output.
for(const elapsed of [0,150,300,450,600,900]){const from=40,to=95,duration=600,t=elapsed/duration;const value=elapsed<=0?from:elapsed>=duration?to:from+(to-from)*(t*t*(3-2*t));assert.ok(lines.includes('elapsed='+elapsed+': Fuel: '+Math.trunc(value)+'%'));}
process.stdout.write(stdout);
