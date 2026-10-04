#!/usr/bin/env python3
"""Cold-process, uncached frontend benchmark; no Cargo/backend compilation."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
HOME = Path.home()
LUNA = HOME / '.cache/jet-dev'
DEFAULT_OUT = HOME / '.cache/jet-dev/scratch/sol/SolJetcBench/results'


def digest(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()


def prepare(row, out):
    root = out / 'inputs' / row['name']
    entry = ('main.jet' if 'generator' in row or row.get('startup') else
             str(Path(row['entry']).relative_to(Path(row['project'])))
             if 'project' in row else Path(row.get('entry', 'main.jet')).name)
    if root.exists():
        return root, entry
    root.mkdir(parents=True)
    if 'generator' in row:
        subprocess.run(['node', str(REPO / 'Tools/perf/scaling/generate.mjs'),
                        row['generator'], str(row['n']), str(root)], check=True)
    elif row.get('startup'):
        (root / entry).write_text('fn run() {}\n')
    elif 'project' in row:
        project = REPO / row['project']
        for source in sorted(project.rglob('*.jet')):
            if any(part.startswith('.') for part in source.relative_to(project).parts):
                continue
            target = root / source.relative_to(project)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, target)
    else:
        shutil.copyfile(REPO / row['entry'], root / entry)
    if not (root / 'package.jet').exists():
        (root / 'package.jet').write_text(
            f'name: "bench_{row["name"].replace("-", "_")}"\n'
            'version: "0.0.1"\nedition: "2028"\n')
    # Record the exact input, not just the live checkout's name.
    provenance = {str(p.relative_to(root)): digest(p) for p in sorted(root.rglob('*.jet'))}
    (root / 'provenance.json').write_text(json.dumps(provenance, indent=2) + '\n')
    return root, entry


def measure(command, label, root, directory, cap, timeout, rust=False, extra_env=None):
    directory.mkdir(parents=True, exist_ok=True)
    metrics = directory / 'time.txt'
    env = {'HOME': str(HOME), 'PATH': os.environ['PATH'],
           'TMPDIR': str(directory), 'LC_ALL': 'C',
           'JET_STORE_DIR': str(directory / 'store'),
           'RUST_MIN_STACK': '8388608', 'RUST_BACKTRACE': '1'}
    (directory / 'store').mkdir()
    env.update(extra_env or {})
    timed = [shutil.which('time'), '-f', '%e\t%M\t%x', '-o', str(metrics),
             'timeout', '-k', '60', str(timeout), *command]
    clean = ['env', '-i', *[f'{key}={value}' for key, value in env.items()], *timed]
    if rust:
        scoped = ['bash', str(LUNA / 'laneB.sh'), '8', *clean]
    else:
        # Match stage1/lib.sh jetc(): explicit environment, 1 GiB main stack,
        # jetwork.slice, no swap, timeout. laneB itself forbids caps above 12G.
        scoped = ['systemd-run', '--user', '--slice=jetwork.slice', '--scope', '-q',
                  f'--unit=jw-jetc-bench-{os.getpid()}-{time.time_ns()}',
                  f'-p', f'MemoryMax={cap}', '-p', 'MemorySwapMax=0',
                  'bash', '-c', 'ulimit -c 0; ulimit -s 1048576; exec "$@"',
                  'jetc-bench', *clean]
    (directory / 'command.json').write_text(json.dumps(scoped, indent=2) + '\n')
    print(f'{root.name}: {label}', file=sys.stderr, flush=True)
    with (directory / 'stdout').open('wb') as stdout, (directory / 'stderr').open('wb') as stderr:
        result = subprocess.run(scoped, cwd=root, stdout=stdout, stderr=stderr)
    data = {'status': result.returncode, 'wall_s': None, 'peak_rss_kib': None}
    if metrics.exists():
        fields = metrics.read_text().splitlines()[-1].split('\t')
        if len(fields) == 3:
            data.update(wall_s=float(fields[0]), peak_rss_kib=int(fields[1]))
    if result.returncode != 0:
        data['error'] = f'process failed; see {directory / "stderr"}'
    return data


def receipt_result(path):
    if not path.exists():
        return 'missing runner receipt'
    values = {}
    reports = []
    for line in path.read_text().splitlines():
        key, _, value = line.partition('=')
        if key == 'report_json':
            reports.append(json.loads(value))
        else:
            values[key] = value
    if values.get('mode') != 'runner' or values.get('complete') != 'true' or int(values.get('source_bytes', '0')) <= 0:
        return f'incomplete runner receipt: {values}'
    errors = [report for report in reports if str(report.get('code', '')).startswith('E')]
    if errors:
        return f'runner reported errors: {errors}'
    return None


def report(rows, results):
    print('workload\tleg\twall_s\tpeak_RSS_MiB\tstatus\tjetc/Rust-emit\tjetc/Rust-check')
    for row in rows:
        legs = results.get(row['name'], {})
        for name, leg in legs.items():
            wall = leg['wall_s']
            ratios = []
            for baseline in ('rust-emit', 'rust-check'):
                ref = legs.get(baseline, {})
                if name.startswith('jetc:') and not leg.get('error') and ref.get('wall_s') and not ref.get('error'):
                    ratios.append(f'{wall / ref["wall_s"]:.3f}')
                else:
                    ratios.append('-')
            rss = leg['peak_rss_kib']
            rss_cell = f'{rss / 1024:.1f}' if rss is not None else '-'
            wall_cell = str(wall) if wall is not None else '-'
            status = 'FAIL' if leg.get('error') else 'OK'
            if name.startswith('jetc:') and ratios[0] != '-':
                status = 'AT-LEAST-AS-FAST' if wall <= legs['rust-emit']['wall_s'] else 'SLOWER'
            print('\t'.join([row['name'], name, wall_cell, rss_cell, status, *ratios]))
    for name, floor in results.get('empty', {}).items():
        hello = results.get('hello', {}).get(name)
        if name.startswith('jetc:') and hello and not floor.get('error') and not hello.get('error'):
            marginal = hello['wall_s'] - floor['wall_s']
            print(f'{name}: empty-program floor={floor["wall_s"]:.2f}s; '
                  f'hello-minus-empty={marginal:.2f}s. '
                  'This includes restore + minimal compile/emission, not an instrumented restore timer.',
                  file=sys.stderr)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('workloads', nargs='*', help='default: hello functions-256; use --all for the full ladder')
    parser.add_argument('--all', action='store_true')
    parser.add_argument('--phase', choices=('both', 'rust', 'jetc', 'report'), default='both')
    parser.add_argument('--rust', type=Path, default=HOME / '.cache/jet-dev/scratch/jet-release-night12/jet')
    parser.add_argument('--jetc', type=Path, action='append', help='repeat to compare builds; default: loop7 when executable, otherwise loop')
    parser.add_argument('--out', type=Path, default=DEFAULT_OUT / time.strftime('%Y%m%d-%H%M%S'))
    parser.add_argument('--timeout', type=int, default=600)
    args = parser.parse_args()
    rows = json.loads((HERE / 'workloads.json').read_text())
    selected = set(args.workloads or ([] if args.all else ['hello', 'functions-256']))
    if selected - {row['name'] for row in rows}:
        parser.error(f'unknown workloads: {sorted(selected - {row["name"] for row in rows})}')
    rows = [row for row in rows if row['name'] in selected or (not selected and not row.get('diagnostic'))]
    out = args.out.expanduser().resolve()
    out.mkdir(parents=True, exist_ok=True)
    results_path = out / 'results.json'
    results = json.loads(results_path.read_text()) if results_path.exists() else {}
    if args.phase == 'report':
        report(rows, results)
        return
    compilers = args.jetc
    if compilers is None:
        candidate = LUNA / 'loop7/jetc0'
        compilers = [candidate if os.access(candidate, os.X_OK) else LUNA / 'loop/jetc0']
    compilers = [p.expanduser().resolve() for p in compilers]
    binaries = ([args.rust.expanduser().resolve()] if args.phase in ('both', 'rust') else [])
    if args.phase in ('both', 'jetc'):
        binaries += compilers
    for binary in binaries:
        if not os.access(binary, os.X_OK):
            parser.error(f'not executable: {binary}')
    # Serialize invocations of this harness; queue time is outside measured time.
    DEFAULT_OUT.parent.mkdir(parents=True, exist_ok=True)
    with (DEFAULT_OUT.parent / 'benchmark.lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        for row in rows:
            root, entry = prepare(row, out)
            legs = results.setdefault(row['name'], {})
            specs = []
            if args.phase in ('both', 'rust'):
                specs += [('rust-check', args.rust.resolve(), ['check', entry], True),
                          ('rust-emit', args.rust.resolve(), ['emit', '--rust', entry], True)]
            if args.phase in ('both', 'jetc'):
                specs += [(f'jetc:{p.parent.name}', p, [], False) for p in compilers]
            for label, binary, flags, rust in specs:
                if label in legs:
                    continue  # A phased/resumed run never silently overwrites evidence.
                directory = out / row['name'] / label.replace(':', '-')
                extra = None if rust else {
                    'JET_BOOTSTRAP_MODE': 'runner', 'JET_BOOTSTRAP_FACTORY_TIER': 'aot',
                    'JET_BOOTSTRAP_SOURCE_ROOT': str(root), 'JET_BOOTSTRAP_ENTRY': entry,
                    'JET_BOOTSTRAP_OUTPUT': str(directory / 'emitted.rs'),
                    'JET_BOOTSTRAP_RECEIPT': str(directory / 'receipt')}
                leg = measure([str(binary), *flags], label, root, directory,
                              '14G', args.timeout, rust=rust, extra_env=extra)
                leg.update(binary=str(binary), binary_size=binary.stat().st_size,
                           binary_mtime_ns=binary.stat().st_mtime_ns,
                           input_provenance=str(root / 'provenance.json'))
                if not rust and leg['status'] == 0:
                    error = receipt_result(directory / 'receipt')
                    if error:
                        leg['error'] = error
                if rust and label == 'rust-emit' and leg['status'] == 0 and not (directory / 'stdout').stat().st_size:
                    leg['error'] = 'Rust emission produced no source'
                legs[label] = leg
                results_path.write_text(json.dumps(results, indent=2) + '\n')
    report(rows, results)
    if any(leg.get('error') for row in rows for leg in results.get(row['name'], {}).values()):
        sys.exit(1)


if __name__ == '__main__':
    main()
