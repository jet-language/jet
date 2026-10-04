#!/usr/bin/env python3
"""Gate contract tests and immutable existing-evidence replays (no compiler run)."""
import hashlib, json, os, pathlib, re, shutil, subprocess, sys, tempfile
here = pathlib.Path(__file__).parent
home = pathlib.Path.home()
out = pathlib.Path(sys.argv[1] if len(sys.argv)>1 else home/'.cache/jet-dev/scratch/PerfGates/gate-tests')
out.mkdir(parents=True, exist_ok=True)
receipts=[]
def run(name, command, expected=None, environment=None):
    env=os.environ.copy();env.update(environment or {})
    p=subprocess.run([str(x) for x in command],env=env,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
    (out/(name+'.log')).write_text(p.stdout)
    receipts.append({'name':name,'command':[str(x) for x in command],'rc':p.returncode,'log':str(out/(name+'.log'))})
    if expected is not None: assert p.returncode in expected,(name,p.returncode,p.stdout)
    print(f'{name}: rc={p.returncode} ({out/(name+".log")})')
    return p

def ladder(root, time_k=1., rss_k=1.):
    root.mkdir(); metadata=['rung\tpackages\tfiles\tunit_bytes\tunit_lines\tunit_sha256']
    for n,size in enumerate((1000,2000,4000),1):
        name='L'+str(n);metadata.append(f'{name}\tfixture\t1\t{size}\t1\ttest')
        d=root/name;d.mkdir()
        seconds=10*(size/1000)**time_k;rss=1000*(size/1000)**rss_k
        (d/'result.env').write_text(f'status=ok\nrc=0\nwall_ms={seconds*1000}\n')
        events=[{'name':'parse','cat':'parse','tid':1,'ph':'B','ts':0},{'name':'parse','cat':'parse','tid':1,'ph':'E','ts':round(seconds*1e6),'args':{'rss_kb':round(rss),'peak_rss_kb':round(rss)}}]
        (d/'trace.json').write_text('[\n'+',\n'.join(json.dumps(e) for e in events)+'\n]\n')
    (root/'rungs.tsv').write_text('\n'.join(metadata)+'\n')
    (root/'ladder.env').write_text('selected=L1,L2,L3\n')

with tempfile.TemporaryDirectory(prefix='gate-contract-',dir=out) as temp:
    temp=pathlib.Path(temp)
    for name,tk,rk,code in [('linear',1.,1.,0),('time-superlinear',1.31,1.,1),('rss-superlinear',1.,2.,1)]:
        d=temp/name;ladder(d,tk,rk);p=run(name,[here/'growth-gate.sh',d],{code})
        if code: assert 'SUPERLINEAR parse '+('rss' if name.startswith('rss') else 'seconds') in p.stdout
    d=temp/'missing-rung';ladder(d);shutil.rmtree(d/'L3');run('missing-rung',[here/'growth-gate.sh',d],{2})
    # Only trap-free runs are fitted: a failed keep-going rung reran parse seven
    # times (7 x 40 s); fitting it would report false superlinear growth.
    d=temp/'trapped-rung';ladder(d)
    (d/'L3'/'result.env').write_text('status=failed\nrc=0\nwall_ms=280000\n')
    events=[e for i in range(7) for e in ({'name':'parse','cat':'parse','tid':1,'ph':'B','ts':i*40_000_000},{'name':'parse','cat':'parse','tid':1,'ph':'E','ts':(i+1)*40_000_000,'args':{'rss_kb':4000,'peak_rss_kb':4000}})]
    (d/'L3'/'trace.json').write_text('[\n'+',\n'.join(json.dumps(e) for e in events)+'\n]\n')
    p=run('trapped-rung-excluded',[here/'growth-gate.sh',d],{2})
    assert 'EXCLUDED L3 parse seconds=280.000000' in p.stdout and 'SUPERLINEAR' not in p.stdout
    d=temp/'exclusive-growth';ladder(d)
    for n in (1,2,3):
        scale=2**(n-1);phase_seconds=1000+10*scale**2
        events=[
            {'name':'compile','cat':'compile','tid':1,'ph':'B','ts':0},
            {'name':'load','cat':'load','tid':1,'ph':'B','ts':0},
            {'name':'load','cat':'load','tid':1,'ph':'E','ts':1000*1e6,'args':{'rss_kb':1000*scale,'peak_rss_kb':1000*scale}},
            {'name':'compile','cat':'compile','tid':1,'ph':'E','ts':phase_seconds*1e6,'args':{'rss_kb':1000*scale,'peak_rss_kb':1000*scale}}]
        (d/f'L{n}'/'trace.json').write_text('[\n'+',\n'.join(json.dumps(e) for e in events)+'\n]\n')
        (d/f'L{n}'/'result.env').write_text(f'status=ok\nrc=0\nwall_ms={(phase_seconds+scale**2)*1000}\n')
    p=run('exclusive-rest-and-startup-growth',[here/'growth-gate.sh',d],{1})
    assert 'SUPERLINEAR compile/(rest) seconds k=2.000000' in p.stdout
    assert 'SUPERLINEAR startup+exit seconds k=2.000000' in p.stdout
    tree=temp/'source';(tree/'Compiler').mkdir(parents=True);(tree/'Core').mkdir()
    bad='''struct Graph { rows: [Int] }
fn scan(rows: [Int], value: Int) -> Bool {
    loop row in rows { if row == value -> return true }
    false
}
fn bad(rows: [Int], graph: Graph) {
    text := ""
    chunks := [String]{}
    copies := [Graph]{}
    loop row in rows {
        loop other in rows -> _ := other
        _ := rows.contains(row)
        text = text + "x"
        text = "{text}x"
        _ := chunks.join("")
        &chunks.push("x" + text)
        copy := graph
        field_copy := graph.rows
        whole := ~rows
        &copies.push(~graph)
        _ := scan(rows, row)
    }
}
'''
    (tree/'Compiler/cases.jet').write_text(bad)
    p=run('static-all-rules',['node',here/'superlinear-static.mjs',tree],{1})
    for rule in ('nested-scan','linear-lookup','prefix-concat','join-in-loop','aggregate-copy','copy-push','repeated-scan'): assert ':'+rule+' ' in p.stdout,rule
    assert 'graph . rows' in p.stdout, 'aggregate field-copy inference'
    assert 'String/list + inside loop' in p.stdout, 'non-assignment concatenation'
    keys=re.findall(r'^FAIL (\S+)',p.stdout,re.M)
    allow=tree/'.superlinear-allowlist.json'
    allow.write_text(json.dumps([{'key':key,'justification':'Test-only deliberately quadratic fixture; validates allowlist matching, not a production exception.'} for key in keys]))
    run('allowlist-justified',['node',here/'superlinear-static.mjs',tree],{0})
    allow.write_text(json.dumps([{'key':keys[0],'justification':''}]))
    run('allowlist-unjustified',['node',here/'superlinear-static.mjs',tree],{2})
    allow.unlink()
    (tree/'Compiler/cases.jet').write_text('''fn linear(rows: [Int]) -> Int {
    total := 0
    // loop fake in rows { rows.contains(fake) }
    loop row in rows -> total = total + row
    total
}
''')
    run('static-linear-control',['node',here/'superlinear-static.mjs',tree],{0})
    for candidate in ('cand24f','cand25f'):
        source=home/'.cache/jet-dev/scratch/ladder'/f'run-{candidate}-L5'
        run(candidate+'-single-rung',[here/'growth-gate.sh',source],{1,2})
        merged=out/(candidate+'-L1-L5');merged.mkdir(exist_ok=True)
        origins=[]
        shutil.copyfile(source/'rungs.tsv',merged/'rungs.tsv')
        (merged/'ladder.env').write_text('selected=L1,L5\n')
        for rung in ('L1','L5'):
            src=home/'.cache/jet-dev/scratch/ladder'/f'run-{candidate}-{rung}'/rung
            shutil.copytree(src,merged/rung,dirs_exist_ok=True)
            for filename in ('result.env','trace.json'):
                origins.append({'path':str(src/filename),'sha256':hashlib.sha256((src/filename).read_bytes()).hexdigest()})
        (merged/'evidence-origin.json').write_text(json.dumps(origins,indent=2)+'\n')
        run(candidate+'-combined-growth',[here/'growth-gate.sh',merged],{1,2})
    run('cand9-growth',[here/'growth-gate.sh',home/'.cache/jet-dev/scratch/ladder/run-cand9'],{2})
    profile=home/'.cache/jet-dev/scratch/PerfAudit-1/regression-runs/c25f-L5'
    run('cand25f-profile-replay',[here/'phase-profile.sh',home/'.cache/jet-dev/cand25f/jetc0',home/'.cache/jet-dev/scratch/ladder/L5/project'],{1},{'PHASE_PROFILE_REPLAY':str(profile),'PHASE_PROFILE_OUT':str(out/'phase-replay')})
    capped=home/'.cache/jet-dev/cand25f/phase-profile-gate-test/run-1791077897315783862'
    p=run('capped-profile-replay',[here/'phase-profile.sh',home/'.cache/jet-dev/cand25f/jetc0',home/'.cache/jet-dev/scratch/ladder/L1/project'],{1},{'PHASE_PROFILE_REPLAY':str(capped),'PHASE_PROFILE_OUT':str(out/'capped-replay')})
    assert re.search(r'^comptime \d+ samples \(OPEN at recording end; partial\)',p.stdout,re.M), 'capped hot phase must retain attributed symbols'
    for path in (out/'capped-replay').glob('run-*/phase-counts.json'):
        data=json.loads(path.read_text())
        assert all(end < float('inf') for _,_,end in data['phase_spans']), 'serialized open spans must have finite sample-end bounds'
        assert data['phases']['comptime']['open_at_recording_end'] and data['phases']['comptime']['samples']>0
    p=run('ladder-direct-cli',['node',here.parent/'ladder.mjs','report',home/'.cache/jet-dev/scratch/ladder/run-cand25f-L5'],{0})
    assert p.stdout.startswith('ladder:'), 'direct CLI must still execute through migrated cache aliases'
if len(sys.argv)>2:
    run('live-static',['node',here/'superlinear-static.mjs',sys.argv[2]],{1},{
        'STATIC_PROFILE':str(home/'.cache/jet-dev/scratch/PerfAudit-1/regression-runs/c25f-L5/phase-counts.json'),
        'STATIC_REPORT':str(home/'.cache/jet-dev/perf/STATIC-SUPERLINEAR.md')})
(out/'receipts.json').write_text(json.dumps(receipts,indent=2)+'\n')
print(f'PASS: {len(receipts)} contract/evidence cases; replay is NOT a new perf recording or a successful compiler proof')
