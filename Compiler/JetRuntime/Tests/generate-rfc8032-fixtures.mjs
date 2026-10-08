#!/usr/bin/env node
// Extract ALL five PURE Ed25519 KATs; ctx/ph are distinct, unasked surfaces.
import {readFileSync,writeFileSync,existsSync} from 'node:fs';
import {resolve,dirname,join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
import {generateEd25519Headers} from './generate-ed25519-headers.mjs';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../../..'),out=join(root,'Compiler/JetRuntime/Tests'),path=process.argv[2];if(!path)throw Error('usage: generate-rfc8032-fixtures.mjs RFC8032.txt');
if(existsSync(join(out,'Ed25519OfficialVectors.json')))throw Error('refusing to overwrite frozen RFC8032 vectors');
const bytes=readFileSync(path),section=bytes.toString('utf8').match(/^7\.1\.\s+Test Vectors for Ed25519\s*\n([\s\S]*?)^7\.2\./m);if(!section)throw Error('missing RFC8032 section7.1');
const hex=text=>text.split('\n').map(s=>s.trim()).filter(s=>/^[0-9a-f]+$/.test(s)).join('');
const cases=section[1].split('-----TEST ').slice(1).map(part=>{const label='rfc_'+part.split('\n')[0].trim().replace(/[^A-Za-z0-9]+/g,'_'),seed=hex(part.split('SECRET KEY:')[1].split('PUBLIC KEY:')[0]),publicKey=hex(part.split('PUBLIC KEY:')[1].split('MESSAGE')[0]),match=part.match(/MESSAGE \(length (\d+) bytes?\):([\s\S]*?)SIGNATURE:/),message=hex(match[2]),signature=hex(part.split('SIGNATURE:')[1]);if(seed.length!==64||publicKey.length!==64||signature.length!==128||message.length!==Number(match[1])*2)throw Error(`invalid ${label} extraction`);return {label,seed,publicKey,message,signature};});if(cases.length!==5)throw Error('RFC8032 section7.1 must contain all five pure Ed25519 KATs');
const hash=b=>createHash('sha256').update(b).digest('hex'),official={url:'https://www.rfc-editor.org/rfc/rfc8032.txt',section:'7.1',sourceSha256:hash(bytes),cases};const json=JSON.stringify(official,null,2)+'\n';writeFileSync(join(out,'Ed25519OfficialVectors.json'),json);
const metadata=join(out,'CryptoEd25519SignatureFixtures.json'),fixture=JSON.parse(readFileSync(metadata,'utf8'));fixture.officialKAT={url:official.url,section:'7.1',path:'Compiler/JetRuntime/Tests/Ed25519OfficialVectors.json',sha256:hash(json),cases:cases.length};writeFileSync(metadata,JSON.stringify(fixture,null,2)+'\n');console.log(`ED25519_OFFICIAL_FROZEN cases=${cases.length} source_sha256=${official.sourceSha256}`);
generateEd25519Headers(out);
