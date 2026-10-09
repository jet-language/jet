#!/usr/bin/env node
// Jet-only self-host MIR observation. One checked program serves every package;
// package slices contain IDs, never independently compiled or Rust-fed MIR.
import {createHash} from 'node:crypto';
import {mkdirSync,readFileSync,writeFileSync} from 'node:fs';
import {basename,dirname,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {loadMirSchema} from './mir-debug.mjs';
import {decodeStream} from './front-feed-codec.mjs';
import {candidateJetc0,runJetMir} from './jet-front.mjs';
const repo=resolve(dirname(fileURLToPath(import.meta.url)),'../../..');
const tsv=value=>String(value).replaceAll('\\','\\\\').replaceAll('\t','\\t').replaceAll('\n','\\n').replaceAll('\r','\\r');
export function selfInventory(program,map,unit){
 const sourceFiles=new Map(program.source_files.map(source=>[source.id.value,source.path]));
 const packages=new Map(),operations=new Map(),functions=[];
 for(const fn of program.functions){
  const source=sourceFiles.get(fn.source_file.value);
  const aggregate=source!==undefined&&(resolve(dirname(unit),source)===resolve(unit)||(sourceFiles.size===1&&basename(source)===basename(unit)));
  const segment=aggregate?map.source_segments.find(row=>Number(fn.span.start)>=row.generated.start&&Number(fn.span.start)<row.generated.end):null;
  const original=segment?.source_file??source??'<missing-source>';
  const generated=aggregate&&map.mir_observer&&Number(fn.span.start)>=map.mir_observer.generated.start&&Number(fn.span.start)<map.mir_observer.generated.end;
  const pkg=original.match(/(?:^|\/)Compiler\/([^/]+)\//)?.[1]??(generated?'Bootstrap':'Unmapped');
  const row={id:fn.id.value,key:fn.key,name:fn.name,source_file_id:fn.source_file.value,span_start:fn.span.start,span_end:fn.span.end,converted:1,package:pkg,source:original};
  functions.push(row);if(!packages.has(pkg))packages.set(pkg,[]);packages.get(pkg).push(row);
  for(const block of fn.blocks)for(const instruction of block.instructions){const operation=instruction.operation.$variant;operations.set(operation,(operations.get(operation)??0)+1);}
 }
 return {functions,sources:program.source_files,packages,operations};
}
function main(){
 const [output,...args]=process.argv.slice(2);if(!output)throw new Error('expected OUT --front jet --unit SELFHOST.jet --map compiler.map.json [--candidate JETC0|--decode MIR.mird]');
 const options={};for(let i=0;i<args.length;i+=2){if(!['--front','--unit','--map','--candidate','--decode'].includes(args[i])||!args[i+1])throw new Error(`invalid option ${args[i]}`);options[args[i].slice(2)]=args[i+1];}
 if(options.front!=='jet'||!options.unit||!options.map)throw new Error('self-host feed requires --front jet, --unit and --map');
 const out=resolve(output),unit=resolve(options.unit),map=JSON.parse(readFileSync(options.map,'utf8'));mkdirSync(out,{recursive:true});
 const mir=join(out,'self.mird'),candidate=options.decode?null:resolve(options.candidate??candidateJetc0());
 if(!options.decode){const problem=runJetMir({candidate,file:unit,root:dirname(unit),cwd:dirname(unit),prefix:join(out,'self'),store:join(out,'store'),mir});if(problem){writeFileSync(join(out,'self.failure.json'),JSON.stringify(problem,null,2)+'\n');console.error(problem.detail);process.exitCode=1;return;}}
 const observed=options.decode?resolve(options.decode):mir;
 const decoded=decodeStream(readFileSync(observed,'utf8'),loadMirSchema(repo));
 const inventory=selfInventory(decoded.program,map,unit);
 const fields=['id','key','name','source_file_id','span_start','span_end','converted'];
 const table=rows=>fields.join('\t')+'\n'+rows.map(row=>fields.map(field=>tsv(row[field])).join('\t')).join('\n')+'\n';
 writeFileSync(join(out,'self.functions.tsv'),table(inventory.functions));
 writeFileSync(join(out,'self.sources.tsv'),'id\tpath\n'+inventory.sources.map(source=>`${source.id.value}\t${tsv(source.path)}`).join('\n')+'\n');
 const slices=[];for(const [pkg,functions] of inventory.packages){const dir=join(out,'slices',pkg);mkdirSync(dir,{recursive:true});writeFileSync(join(dir,'functions.tsv'),table(functions));slices.push({package:pkg,functions:functions.length,ids:functions.map(fn=>fn.id),program:observed});}
 const receipt={front:'jet',candidate,program:observed,program_sha256:createHash('sha256').update(readFileSync(observed)).digest('hex'),unit,unit_sha256:createHash('sha256').update(readFileSync(unit)).digest('hex'),map:resolve(options.map),map_sha256:createHash('sha256').update(readFileSync(options.map)).digest('hex'),entry:decoded.entry,schema:decoded.program.schema_version,functions:inventory.functions.length,slices,operations:Object.fromEntries([...inventory.operations].sort(([a],[b])=>a.localeCompare(b))),native_execution:false};
 writeFileSync(join(out,'self.inventory.json'),JSON.stringify(receipt,null,2)+'\n');console.log(`Jet-front MIR inventory: ${receipt.functions} functions, ${slices.length} packages, ${inventory.operations.size} operation variants (not native execution)`);
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url))main();
