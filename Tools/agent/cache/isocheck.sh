#!/usr/bin/env bash
JET_AGENT_TOOLS="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
JET_REPO="${JET_REPO:-$(cd "$JET_AGENT_TOOLS/../.." && pwd)}"
# Isolated check of the assembled Jet compiler for ONE worker's edits.
# Base = the Compiler/ tree of the last clean unitcheck (~/.cache/jet-dev/unitcheck/good-src),
# overlaid with only the files you name from the live tree. Other workers' half-saved edits
# cannot break your check, and yours cannot break theirs.
#
# usage: isocheck.sh <worker-name> <repo-relative path>...
#   Name every file you changed or created (and Compiler/Bootstrap/sources.list if you edited it).
#   Deleted files: name them too; they are removed from your copy.
# result: ~/.cache/jet-dev/iso/<worker>/latest.txt (also printed); errors list unit lines and,
#         when possible, the source file:line they come from.
# Runs in the unitcheck lane (holds ~/.cache/jet-dev/unitcheck/lock); run it detached.
set -u
[ $# -ge 1 ] || { echo "usage: isocheck.sh <worker> <path>..."; exit 64; }
W=$1; shift
R=$JET_REPO
L=$HOME/.cache/jet-dev
BASE=${ISOCHECK_BASE:-$L/unitcheck/good-src}
I=$L/iso/$W
mkdir -p "$I"
rm -rf "$I/root" "$I/home"; mkdir -p "$I/root" "$I/home"
if [ -d "$BASE/Compiler" ]; then cp -r "$BASE/Compiler" "$I/root/Compiler"; else cp -r "$R/Compiler" "$I/root/Compiler"; fi
for p in "$@"; do
  if [ -e "$R/$p" ]; then mkdir -p "$I/root/$(dirname "$p")"; cp "$R/$p" "$I/root/$p"; else rm -f "$I/root/$p"; fi
done
# files listed in the (possibly overlaid) manifest but absent from the base: take them from the live tree
python3 - "$I/root" "$R" <<'EOF'
import sys,os,shutil
root,live=sys.argv[1],sys.argv[2]
for l in open(os.path.join(root,'Compiler/Bootstrap/sources.list')):
    p=l.strip()
    if not p or p.startswith('#'): continue
    if not os.path.exists(os.path.join(root,p)) and os.path.exists(os.path.join(live,p)):
        os.makedirs(os.path.dirname(os.path.join(root,p)),exist_ok=True); shutil.copy(os.path.join(live,p),os.path.join(root,p))
        print('pulled from live:',p)
EOF
cd "$I/root" && HOME="$I/home" node Compiler/Bootstrap/assemble.mjs > "$I/assemble.log" 2>&1 || { echo "$(date +%T) assemble FAILED: $(tail -3 "$I/assemble.log")" | tee "$I/latest.txt"; exit 1; }
P=$I/home/.cache/jet-luna/compiler-bootstrap/project
JETBIN=${ISOCHECK_JET:-$( [ -x $HOME/.cache/jet-dev/scratch/jet-release-night/jet ] && echo $HOME/.cache/jet-dev/scratch/jet-release-night/jet || echo $HOME/.cache/jet-dev/scratch/jet-current)}
exec 9>"$L/unitcheck/lock"; flock 9
cd "$P" && JET_RECEIPT_BYPASS=1 JET_STORE_DIR=$I/store SAFE_JET_MEM=${ISOCHECK_MEM:-14G} SAFE_JET_TIMEOUT=1200 JET=$JETBIN \
  "$JET_AGENT_TOOLS/cache/safe-jet.sh" check --color=never src/compiler.jet > "$I/check.log" 2>&1
rc=$?
python3 - "$I/check.log" "$rc" "$P/src/compiler.jet" > "$I/latest.txt" <<'EOF'
import sys,re,collections,time,bisect
t=open(sys.argv[1],errors='ignore').read()
errs=re.findall(r'^Error \[(\w+)\][^\n]*\n\s*-->\s*src/compiler\.jet:(\d+):(\d+)',t,re.M)
print(time.strftime('%H:%M:%S'),'rc='+sys.argv[2],'errors='+str(len(errs)))
print(dict(collections.Counter(e[0] for e in errs)))
# map unit lines back to source files via the assembler's file markers, if present
unit=open(sys.argv[3],errors='ignore').read().split('\n')
marks=[]
for i,l in enumerate(unit):
    m=re.match(r'^// \[jet-bootstrap source: (\S+?)\]',l)
    if m: marks.append((i+1,m.group(1)))
starts=[m[0] for m in marks]
for code,line,col in errs[:60]:
    n=int(line); where=''
    k=bisect.bisect_right(starts,n)-1
    if k>=0: where=f'  ({marks[k][1]}:{n-marks[k][0]})'
    print(code,f'unit:{n}:{col}',where,'|',unit[n-1].strip()[:90] if n-1<len(unit) else '')
if sys.argv[2] in ('137','124'): print('KILLED (memory or timeout)')
EOF
cat "$I/latest.txt"
