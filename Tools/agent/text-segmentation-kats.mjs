#!/usr/bin/env node
// #4820: observed Rust oracle -> full Unicode 17 UAX #29 public Core KATs.
// --write generates the corpus programs; --check checks their exact bytes.
// --jet <fresh-rebuilt-jet> additionally runs the source-port differential.
// Scratch is external; no Rust runtime or dependency is added to Jet.
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
import assert from 'node:assert/strict';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const args = process.argv.slice(2);
const check = args.includes('--check');
const jetAt = args.indexOf('--jet');
const jet = jetAt < 0 ? null : args[jetAt + 1];
const scratch = process.env.JET_TEST_SCRATCH_DIR;
assert(scratch && path.isAbsolute(scratch) && !scratch.startsWith('/tmp/'), 'set external JET_TEST_SCRATCH_DIR');
mkdirSync(scratch, {recursive:true});
function run(command, argv, input) {
  const result = spawnSync(command, argv, {cwd:root, input, encoding:'utf8', maxBuffer:32*1024*1024});
  writeFileSync(path.join(scratch, `${path.basename(command)}.stderr`), result.stderr ?? '');
  assert.equal(result.status, 0, `${command} exit=${result.status}: ${result.error?.message ?? ''}\n${result.stderr}`);
  return result.stdout;
}
const textPath = 'crates/jet-codegen/src/Prelude/CoreLib/Top/Text.rs';
const source = readFileSync(path.join(root,textPath),'utf8');
// Extract only the actual reference definitions, not a JS reimplementation.
function definition(name, text = source) {
  const start = text.search(new RegExp(`\\bfn ${name}(?:<|\\()`));
  assert(start >= 0, `missing reference ${name}`);
  const open = text.indexOf('{',start);
  let depth = 1, end = open + 1;
  while (depth && end < text.length) { const c = text[end++]; if(c === '{') depth++; if(c === '}') depth--; }
  assert.equal(depth,0);
  return text.slice(start,end);
}
const names = ['jet_text_bsearch_pair','jet_text_bsearch_triple','jet_text_property',
  'jet_text_alphabetic','jet_text_numeric','jet_text_word_class','jet_text_sentence_class',
  'jet_text_is_ext_pictographic','jet_text_word_reduce','jet_text_word_break_at',
  'jet_text_word_segments','jet_text_words','jet_text_sentence_breaks',
  'jet_text_sentence_segments','jet_text_sentences','jet_text_trim'];
const constants = [...source.matchAll(/^const JET_(?:WB|SB)_\w+: u8 = \d+;$/gm)].map(m=>m[0]).join('\n');
const stringSource = readFileSync(path.join(root,'crates/jet-codegen/src/Prelude/Core/UnicodeString.rs'),'utf8');
const whitespace = stringSource.match(/^pub static UNICODE_WHITE_SPACE:.*;$/m)?.[0];
assert(whitespace, 'missing pinned White_Space table');
const stringFns = ['jet_unicode_contains','jet_unicode_trim_view','jet_unicode_trim'].map(name=>definition(name,stringSource)).join('\n');
const oracle = `#![allow(dead_code)]\ninclude!(${JSON.stringify(path.join(root,'crates/jet-codegen/src/Prelude/CoreLib/Top/UnicodeTables.rs'))});\n${whitespace}\n${stringFns}\n${constants}\n${names.map(name=>definition(name)).join('\n')}\n` + String.raw`
fn unhex(s: &str) -> String {
    let bytes: Vec<u8> = (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i+2],16).unwrap()).collect();
    String::from_utf8(bytes).unwrap()
}
fn hex(s: &str) -> String { s.as_bytes().iter().map(|b| format!("{b:02x}")).collect() }
fn main() {
    use std::io::BufRead;
    let mut count = 0;
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap();
        let fields: Vec<_> = line.split('\t').collect();
        let input = unhex(fields[2]);
        let expected: Vec<String> = fields[3].split(',').map(unhex).collect();
        let (segments, public) = if fields[0] == "word" {
            (jet_text_word_segments(&input), jet_text_words(&input))
        } else {
            (jet_text_sentence_segments(&input), jet_text_sentences(&input))
        };
        assert_eq!(segments, expected, "{} corpus line {}", fields[0], fields[1]);
        println!("{}\t{}\t{}\t{}\t{}", fields[0], fields[1], fields[2], segments.iter().map(|s|hex(s)).collect::<Vec<_>>().join(","), public.iter().map(|s|hex(s)).collect::<Vec<_>>().join(","));
        count += 1;
    }
    eprintln!("Rust UAX29 corpus boundaries verified: {count}");
}
`;
const oraclePath = path.join(scratch,'oracle.rs');
const oracleBin = path.join(scratch,'oracle');
writeFileSync(oraclePath,oracle);
run('rustc',['--edition=2021','-O',oraclePath,'-o',oracleBin]);
const corpora = [
  ['word','WordBreakTest.txt','1de23a75f37904abc7d206239ee8d34f8fdf0fb4ab32a7174dfbabbde25419b2',1944],
  ['sentence','SentenceBreakTest.txt','12cb47d028ded0c1cb8a28558f95479cbcd24559c46977015c82f3b50a1cc6e4',512],
];
const hex = s => Buffer.from(s,'utf8').toString('hex');
const inputs = [];
for(const [kind,file,pin,count] of corpora) {
  const bytes=readFileSync(path.join(root,'tests/data/unicode',file));
  assert.equal(createHash('sha256').update(bytes).digest('hex'),pin);
  let cases = 0;
  for(const [index,raw] of bytes.toString('utf8').split('\n').entries()) {
    const line = raw.split('#')[0].trim();
    if(!line) continue;
    const segments=[]; let current='', input='';
    for(const token of line.split(/\s+/)) {
      if(token === '÷') { if(current) {segments.push(current); current='';} }
      else if(token !== '×') { const ch=String.fromCodePoint(parseInt(token,16)); current+=ch; input+=ch; }
    }
    if(current) segments.push(current);
    inputs.push(`${kind}\t${index+1}\t${hex(input)}\t${segments.map(hex).join(',')}`);
    cases++;
  }
  assert.equal(cases,count);
}
writeFileSync(path.join(scratch,'oracle-input.tsv'),inputs.join('\n')+'\n');
const observed=run(oracleBin,[],inputs.join('\n')+'\n');
writeFileSync(path.join(scratch,'oracle-output.tsv'),observed);
// Every Jet literal is UTF-8 bytes observed from the independent Rust oracle.
// Jet has no Unicode-scalar escape: controls use D-BYTELIT1 byte literals.
function literal(hexString) {
  const s=Buffer.from(hexString,'hex').toString('utf8');
  if([...s].some(c=>c.codePointAt(0)<32 || (c.codePointAt(0)>=127 && c.codePointAt(0)<=159))) {
    const bytes=hexString.match(/../g).map(b=>`\\x${b.toUpperCase()}`).join('');
    return `(String.from_bytes([U8]{"${bytes}"}) ?? panic("invalid oracle UTF-8"))`;
  }
  let out='"';
  for(const c of s) {
    if(c==='"' || c==='\\') out+='\\'+c;
    else if(c==='{') out+='{{';
    else if(c==='}') out+='}}';
    else out+=c;
  }
  return out+'"';
}
const rows=observed.trimEnd().split('\n').map(line=>line.split('\t'));
for(const [kind,, ,count] of corpora) {
  const selected=rows.filter(row=>row[0]===kind);
  assert.equal(selected.length,count);
  const publicFn=kind==='word'?'words':'sentences';
  const rel=`tests/conformance/corpus/core/text/${publicFn}_uax29.jet`;
  const assertions=selected.map(([,line,input,,expected])=>{
    const values=expected?expected.split(',').map(literal):[];
    return `    // UAX #29 ${kind} corpus line ${line}\n    assert_eq(text.${publicFn}(${literal(input)}), [String]{${values.join(', ')}})`;
  });
  const generated=`// Generated by Tools/agent/text-segmentation-kats.mjs from observed Rust Text.rs.\n// Unicode 17.0.0; all ${count} ${kind} corpus cases; no handwritten expectations.\nuse core.text as text\n\nfn run() {\n${assertions.join('\n')}\n    print("${count} ${publicFn} UAX29 KATs passed")\n}\n`;
  if(check) assert.equal(readFileSync(path.join(root,rel),'utf8'),generated,`${rel} is stale`);
  else writeFileSync(path.join(root,rel),generated);
  if(jet) {
    const actual=run(jet,['run',rel]);
    assert.equal(actual,`${count} ${publicFn} UAX29 KATs passed\n`,`${publicFn} source differential`);
    writeFileSync(path.join(scratch,`${publicFn}-differential.stdout`),actual);
  }
}
const receipt={cases:rows.length,word:1944,sentence:512,rustBoundaries:'byte-identical to corpus',
  publicExpectedSha256:createHash('sha256').update(observed).digest('hex'),sourceDifferential:jet?'PASS':'UNEXERCISED',
  reference:textPath,unicode:'17.0.0'};
writeFileSync(path.join(scratch,'receipt.json'),JSON.stringify(receipt,null,2)+'\n');
console.log(JSON.stringify(receipt));
