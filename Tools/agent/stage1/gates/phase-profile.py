#!/usr/bin/env python3
"""Bounded native candidate profile. Partial compiles retain evidence and fail."""
import fcntl, json, os, pathlib, shutil, subprocess, sys, time

def main():
    compiler = pathlib.Path(sys.argv[1]).resolve(strict=True)
    project = pathlib.Path(sys.argv[2]).resolve(strict=True)
    if not os.access(compiler, os.X_OK): raise ValueError('compiler is not executable')
    if not (project/'src/compiler.jet').is_file() or not (project/'package.jet').is_file(): raise ValueError('rung-project must contain package.jet and src/compiler.jet')
    home = pathlib.Path.home()
    base = pathlib.Path(os.environ.get('PHASE_PROFILE_OUT', str(compiler.parent/'phase-profiles'/project.parent.name))).resolve()
    out = base/f'run-{time.time_ns()}'
    out.mkdir(parents=True)
    perf = os.environ.get('PERF', '/nix/store/v5fs9a5z17j8pfd2kfxs44hv5p3yq6ap-perf-linux-7.0.11/bin/perf')
    replay = os.environ.get('PHASE_PROFILE_REPLAY')
    if replay:
        # Explicit evidence replay is only a summarizer test, never a new recording.
        original = pathlib.Path(replay).resolve(strict=True)
        for name in ('clock','phases.trace','perf.script','done.json'):
            shutil.copyfile(original/name, out/name)
        result = json.loads((out/'done.json').read_text())
        result['replayed_from'] = str(original)
    else:
        hold = home/'.cache/jet-dev/scratch/EndToEndQA/frontend-hold'
        if hold.exists(): raise ValueError(f'proof window closed: {hold}')
        cap = int(os.environ.get('PHASE_PROFILE_MEM_GIB','20'))
        seconds = int(os.environ.get('PHASE_PROFILE_SECONDS','1200'))
        if cap < 1 or cap > 20 or seconds < 1 or seconds > 1200: raise ValueError('cap must be 1..20 GiB and duration 1..1200 seconds')
        with (home/'.cache/jet-dev/jetc0-run.lock').open('a') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            if hold.exists(): raise ValueError(f'proof window closed: {hold}')
            available = next(int(x.split()[1]) for x in pathlib.Path('/proc/meminfo').read_text().splitlines() if x.startswith('MemAvailable:'))
            if available < (cap+2)*1024*1024: raise ValueError(f'need {cap+2} GiB available, have {available/1024/1024:.2f}')
            private = out/'project'
            shutil.copytree(project, private, ignore=shutil.ignore_patterns('.jet'), dirs_exist_ok=False)
            environment = os.environ.copy()
            environment.update({'JET_TRACE_FILE':str(out/'phases.trace'), 'JET_BOOTSTRAP_MODE':'runner', 'JET_BOOTSTRAP_FACTORY_TIER':'aot', 'JET_BOOTSTRAP_SOURCE_ROOT':str(private), 'JET_BOOTSTRAP_ENTRY':'src/compiler.jet', 'JET_BOOTSTRAP_OUTPUT':str(out/'out.rs'), 'JET_BOOTSTRAP_RECEIPT':str(out/'receipt'), 'JET_STORE_DIR':str(out/'store'), 'RUST_MIN_STACK':'8388608','TMPDIR':str(out),'LC_ALL':'C'})
            (out/'clock').write_text(f'{time.time()} {time.clock_gettime(time.CLOCK_MONOTONIC)}\n')
            command = ['systemd-run','--user','--slice=jetwork.slice','--scope','-q','-p',f'MemoryMax={cap}G','-p','MemorySwapMax=0','timeout','-k','10',str(seconds),perf,'record','-o',str(out/'perf.data'),'-k','CLOCK_MONOTONIC','-m','64','-F','199','-g','--',str(compiler)]
            with (out/'compile.log').open('w') as log:
                rc = subprocess.run(command,cwd=private,env=environment,stdout=log,stderr=subprocess.STDOUT).returncode
        with (out/'perf.script').open('w') as stream, (out/'perf-script.log').open('w') as errors:
            prc = subprocess.run([perf,'script','-i',str(out/'perf.data'),'-F','time,ip,sym'],stdout=stream,stderr=errors).returncode
        result = {'rc':rc,'perf_script_rc':prc,'compiler':str(compiler),'rung_project':str(project),'MemoryMax':f'{cap}G','timeout_seconds':seconds,'initial_available_kib':available,'command':command}
    (out/'done.json').write_text(json.dumps(result,indent=2)+'\n')
    print(f"profile: {out}; compiler rc={result['rc']} ({'partial/FAILED' if result['rc'] else 'completed'}); {'REPLAY ONLY' if replay else 'RECORDED'}", flush=True)
    if result.get('perf_script_rc') != 0: raise ValueError('perf script failed; see perf-script.log')
    with (out/'top-20.txt').open('w') as stream:
        stream.write(f"compiler rc={result['rc']}; {'REPLAY ONLY' if replay else 'RECORDED'}; failed/capped runs are partial evidence, not throughput acceptance\n")
        stream.flush()
        src = pathlib.Path(__file__).with_name('profile-phases.py')
        summary_rc = subprocess.run([sys.executable,str(src),str(out)],stdout=stream).returncode
    print((out/'top-20.txt').read_text(), end='')
    if summary_rc: raise ValueError('phase attribution failed')
    counts = json.loads((out/'phase-counts.json').read_text())
    if not counts['phases'] or not any(c['samples'] for c in counts['phases'].values()): raise ValueError('no phase-attributed samples')
    if not replay:
        receipt = dict(line.split('=',1) for line in (out/'receipt').read_text().splitlines() if '=' in line) if (out/'receipt').exists() else {}
        if result['rc'] or receipt.get('complete') != 'true': return 1
    return 1 if result['rc'] else 0

if __name__ == '__main__':
    try: sys.exit(main())
    except (OSError,ValueError,KeyError,StopIteration) as e:
        print('UNAVAILABLE: '+str(e),file=sys.stderr);sys.exit(2)
