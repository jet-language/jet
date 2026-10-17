#!/usr/bin/env node
// Render committed frozen JSON, never rerun a Rust/independent oracle.
// Pattern-based sign messages avoid oversized ISO C string literals.
import {readFileSync,writeFileSync,existsSync} from 'node:fs';
import {resolve,dirname,join} from 'node:path';
import {fileURLToPath} from 'node:url';
export function generateEd25519Headers(out){
    const fixture=JSON.parse(readFileSync(join(out,'CryptoEd25519SignatureFixtures.json'),'utf8'));
    const render=c=>{const pattern=c.label.startsWith('sign_')?c.message.length/2:-1,message=pattern>=0?'':c.message;return ` {"${c.label}","${c.seed}","${c.publicKey}","${message}","${c.signature}",${pattern},${c.decoded??1},${c.ordinary??1},${c.strict??1}},`;};
    writeFileSync(join(out,'CryptoEd25519SignatureGoldens.h'),'// ONCE original dalek 2.2.0 matrix; pattern_length>=0 means input(i*197+23)%256.\nstatic const struct Ed25519Golden {const char *label,*seed,*public_key,*message,*signature;int64_t pattern_length;int decoded,ordinary,strict;} ed25519_goldens[]={\n'+fixture.cases.map(render).join('\n')+'\n};\n');
    const official=join(out,'Ed25519OfficialVectors.json');if(existsSync(official))writeFileSync(join(out,'Ed25519OfficialGoldens.h'),'// ALL RFC8032 section7.1 pure Ed25519 KATs, original literal messages.\nstatic const struct Ed25519Golden ed25519_official[]={\n'+JSON.parse(readFileSync(official,'utf8')).cases.map(render).join('\n')+'\n};\n');
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){
    generateEd25519Headers(dirname(fileURLToPath(import.meta.url)));
    console.log('ED25519_HEADERS_FROM_FROZEN_JSON no_oracle_capture');
}
