#!/usr/bin/env python3
"""Attribute perf samples to native trace phases using paired wall/monotonic clocks."""
import collections,json,pathlib,re,sys
p=pathlib.Path(sys.argv[1]); wall,mono=map(float,(p/'clock').read_text().split())
events=[]
for line in (p/'phases.trace').read_text().splitlines():
 line=line.strip().rstrip(',')
 if line.startswith('{'): events.append(json.loads(line))
start=next(e['args']['unix_ms']/1000 for e in events if e.get('name')=='trace.start')-wall+mono
stacks=collections.defaultdict(list);spans=[]
for e in events:
 key=(e.get('tid'),e.get('cat') or e['name']);kind=e.get('ph')
 if kind=='B':stacks[key].append(e['ts'])
 if kind=='E' and stacks[key]:
  a=stacks[key].pop();spans.append((key[1],start+a/1e6,start+e['ts']/1e6))
open_names=set()
for (_,name),begins in stacks.items():
 for a in begins:
  open_names.add(name);spans.append((name,start+a/1e6,float('inf')))
selected=spans
counts={name:{'total':0,'leaf':collections.Counter(),'inclusive':collections.Counter(),'jet_inclusive':collections.Counter()} for name,_,_ in selected}
# perf script emits timestamp-ordered samples. Sweep boundaries once rather than
# rescan every source-size-dependent span for every sample; refcounts union repeats.
edges=sorted([(a,0,name) for name,a,_ in selected]+[(b,1,name) for name,_,b in selected])
edge=0;active=collections.defaultdict(int)
frame=[];timestamp=None
header=re.compile(r'^(\d+\.\d+):\s*$')
jet_fn=re.compile(r'^(?:\w+::)*__jet_src_c_csrc_scompiler_djet_c_c(\w+)$')
def flush():
 global edge
 if timestamp is None or not frame:return
 while edge<len(edges) and edges[edge][0]<=timestamp:
  _,kind,name=edges[edge];active[name]+=1 if kind==0 else -1
  if active[name]==0:del active[name]
  edge+=1
 symbols=set(frame);names={m[1] for symbol in frame if (m:=jet_fn.match(symbol))}
 for name in active:
  if name in counts:
   c=counts[name];c['total']+=1;c['leaf'][frame[0]]+=1
   for symbol in symbols:c['inclusive'][symbol]+=1
   for symbol in names:c['jet_inclusive'][symbol]+=1
for line in (p/'perf.script').open(errors='replace'):
 m=header.match(line)
 if m:flush();timestamp=float(m[1]);frame=[];continue
 if not line.strip():flush();frame=[];continue
 bits=line.strip().split(' ',1)
 if len(bits)==2:frame.append(bits[1].split(' (')[0])
flush()
selected=[(name,a,b if b!=float('inf') else (timestamp if timestamp is not None else a)+1e-6) for name,a,b in selected]
result={'alignment_monotonic_s':start,'phase_spans':selected,'phases':{}}
for name,c in counts.items():
 n=c['total'];r={'samples':n,'open_at_recording_end':name in open_names,'leaf':c['leaf'].most_common(30),'inclusive':c['inclusive'].most_common(80),'jet_inclusive':c['jet_inclusive'].most_common(100)};result['phases'][name]=r
 print(name,n,'samples', '(OPEN at recording end; partial)' if name in open_names else '')
 for mode in ('jet_inclusive','leaf','inclusive'):
  print(mode, 'top 20')
  for symbol,count in r[mode][:20]:print(f'{100*count/n if n else 0:6.2f}% {symbol}')
(p/'phase-counts.json').write_text(json.dumps(result,indent=2)+'\n')
