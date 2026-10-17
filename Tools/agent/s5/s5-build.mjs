#!/usr/bin/env node
// Consume the sole shared host/candidate build and Jet-only pack publication.
// Invalid or absent prerequisite evidence is BLOCKED, never a native pass.
import fs from 'node:fs';
import path from 'node:path';
import {phase, W} from './s5-lib.mjs';
import {readSharedBuild, readSharedPack} from './shared-publications.mjs';

const P = phase('build');
let artifacts = null;
try {
  artifacts = await readSharedBuild({file: process.env.S5_SHARED_ARTIFACTS, head: P.head, source: W, target: process.env.CARGO_TARGET_DIR});
  P.receipt('shared-build-artifacts', 'PASS', {receipt: process.env.S5_SHARED_ARTIFACTS, artifacts: artifacts.artifacts});
} catch (error) { P.blocked('shared-build-artifacts', error.message); }
if (artifacts) {
  fs.symlinkSync(artifacts.artifacts.candidate_jetc0.path, path.join(P.D, 'jetc0'));
  try {
    const publication = readSharedPack({directory: process.env.S5_SHARED_PACK_DIRECTORY, head: P.head, source: W});
    if (publication.receipt.candidate.sha256 !== artifacts.artifacts.candidate_jetc0.sha256) throw Error('shared runtime CLI does not match the Jet-hosted candidate publication');
    fs.symlinkSync(publication.jet, path.join(P.D, 'jet'));
    P.receipt('shared-native-cli', 'PASS', {path: publication.jet, sha256: publication.receipt.candidate.sha256, runtimeKey: publication.key, packSha256: publication.receipt.packSha256});
    fs.writeFileSync(path.join(P.D, 'artifact.json'), JSON.stringify(artifacts, null, 2) + '\n');
  } catch (error) { P.blocked('shared-native-cli', error.message); }
} else P.blocked('shared-native-cli', 'shared build did not publish successful same-pin artifacts');
P.finish();
