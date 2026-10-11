#!/usr/bin/env node
// Provenance guard of Tools/stage0-repack/repack.sh.
//
//   provenance.mjs guard <repo> <emission-rev> <generator-rev> <source-root> <crates> <out.json> [<resume-rev>]
//     Refuses (exit 3) unless `git diff --name-only <emission> <generator>`
//     touches only Compiler/Bootstrap/Host/**, Compiler/Bootstrap/Tests.rs and
//     Tools/**, the kept run's crate tree <crates> is the emission pin, and
//     <source-root> holds exactly the generator pin's Compiler/Bootstrap files.
//     <resume-rev> names the tree a later `JET_STAGE_ZERO_RESUME` builds the
//     backend crates from; its diff from the emission pin may also touch the
//     libjet_codegen copy of the Observe prelude (crates/jet-codegen/src/lib.rs,
//     crates/jet-codegen/src/Prelude/Observe.rs and the jet-codegen and
//     jet-comptime build scripts that set its cfg), which the repack never
//     reads, and is recorded with hashes.
//   provenance.mjs finish <guard.json> <kept-dir> <copy-dir>
//     Compares the repacked copy with the kept dir and writes
//     <copy-dir>/repack-provenance.json. Refuses (exit 3) when anything but
//     stage-zero.rs, stage-zero/generated.rs and stage-zero/src/main.rs changed,
//     or when the unit/shard crate inputs changed: Tests.rs
//     `split_backend_workspace` builds the runtime and unit crates from the
//     runtime block and the `// jet:unit-*` sections, which all lie in the
//     reference.rs prefix of stage-zero.rs.
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

const ALLOWED = [/^Compiler\/Bootstrap\/Host\//, /^Compiler\/Bootstrap\/Tests\.rs$/, /^Tools\//];
const RESUME_ALLOWED = [
  ...ALLOWED,
  /^crates\/jet-codegen\/build\.rs$/,
  /^crates\/jet-codegen\/src\/lib\.rs$/,
  /^crates\/jet-codegen\/src\/Prelude\/Observe\.rs$/,
  /^crates\/jet-comptime\/build\.rs$/,
];
const PACKAGED = ['stage-zero.rs', 'stage-zero/generated.rs', 'stage-zero/src/main.rs'];
// Files the harness itself adds to the copy.
const HARNESS_OUTPUTS = [/^repack\//, /^stage-zero\.base\.rs$/, /^stage-zero\/main\.prefix\.rs$/, /^artifact-main\.rs$/];
const SPLIT_MARKERS = ['// jet:unit-begin ', '// jet:unit-end ', '// jet:cached-runtime-', '// jet:cached-core-'];

const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const fail = message => {
  console.error(`repack provenance: ${message}`);
  process.exit(3);
};
const [mode, ...args] = process.argv.slice(2);

if (mode === 'guard') {
  const [repo, emissionRev, generatorRev, sourceRoot, crates, out, resumeRev] = args;
  if (!out) fail('usage: provenance.mjs guard <repo> <emission-rev> <generator-rev> <source-root> <crates> <out.json> [<resume-rev>]');
  const git = (...argv) => execFileSync('git', ['-C', repo, ...argv], { maxBuffer: 1 << 30 });
  const commit = rev => {
    try {
      return git('rev-parse', '--verify', '--quiet', `${rev}^{commit}`).toString().trim();
    } catch {
      return fail(`\`${rev}\` is not a commit in ${repo}`);
    }
  };
  const blob = (rev, file) => {
    try {
      return git('cat-file', 'blob', `${rev}:${file}`);
    } catch {
      return null;
    }
  };
  const emission = commit(emissionRev);
  const generator = commit(generatorRev);
  const changed = git('diff', '--name-only', '--no-renames', emission, generator).toString().split('\n').filter(Boolean);
  const outside = changed.filter(file => !ALLOWED.some(pattern => pattern.test(file)));
  if (outside.length) {
    fail(
      `refusing: ${emission}..${generator} changes files outside Compiler/Bootstrap/Host/**, ` +
        `Compiler/Bootstrap/Tests.rs and Tools/**:\n  ${outside.join('\n  ')}`,
    );
  }
  // The kept run's crates (image codec, emitter) must be the emission pin's.
  const manifest = path.join(path.dirname(crates), 'source.manifest.json');
  let cratesCommit;
  if (fs.existsSync(manifest)) {
    cratesCommit = JSON.parse(fs.readFileSync(manifest, 'utf8')).commit;
  } else {
    try {
      cratesCommit = execFileSync('git', ['-C', crates, 'rev-parse', 'HEAD']).toString().trim();
    } catch {
      fail(`cannot establish the commit of the kept run's crates ${crates} (no ${manifest}, not a git checkout)`);
    }
  }
  if (cratesCommit !== emission) fail(`the kept run's crates ${crates} are ${cratesCommit}, not the emission pin ${emission}`);
  // Every generator input the harness can read must be the generator pin's.
  const inputs = git('ls-tree', '-r', '--name-only', generator, 'Compiler/Bootstrap').toString().split('\n').filter(Boolean);
  const mismatched = inputs.filter(file => {
    const target = path.join(sourceRoot, file);
    return !fs.existsSync(target) || sha256(fs.readFileSync(target)) !== sha256(blob(generator, file));
  });
  if (mismatched.length) fail(`source root ${sourceRoot} is not an export of ${generator}:\n  ${mismatched.join('\n  ')}`);
  const changedFiles = (rev, files) =>
    files.map(file => {
      const before = blob(emission, file);
      const after = blob(rev, file);
      return { path: file, emission_sha256: before && sha256(before), pin_sha256: after && sha256(after) };
    });
  let resume = null;
  if (resumeRev) {
    const pin = commit(resumeRev);
    const files = git('diff', '--name-only', '--no-renames', emission, pin).toString().split('\n').filter(Boolean);
    const outsideResume = files.filter(file => !RESUME_ALLOWED.some(pattern => pattern.test(file)));
    if (outsideResume.length) {
      fail(`refusing: the RESUME tree ${pin} changes files beyond the generators and the Observe prelude:\n  ${outsideResume.join('\n  ')}`);
    }
    resume = {
      resume_pin: pin,
      diff_name_only: files,
      changed_files: changedFiles(pin, files),
      note: 'RESUME builds the backend crates (libjet_codegen) from this tree; the repack reads only the generator pin',
    };
  }
  fs.writeFileSync(
    out,
    JSON.stringify(
      {
        emission_pin: emission,
        generator_pin: generator,
        source_root: sourceRoot,
        kept_crates: crates,
        diff_name_only: changed,
        changed_files: changedFiles(generator, changed),
        generator_inputs_verified: inputs.length,
        resume_tree: resume,
      },
      null,
      2,
    ) + '\n',
  );
  console.error(
    `repack provenance: ${emission.slice(0, 12)}..${generator.slice(0, 12)} changes ${changed.length} allowed files; ` +
      `${inputs.length} Compiler/Bootstrap files of the source root match the generator pin` +
      (resume ? `; RESUME tree ${resume.resume_pin.slice(0, 12)} differs in ${resume.diff_name_only.length} files` : ''),
  );
} else if (mode === 'finish') {
  const [guardPath, kept, copy, emissionOnly] = args;
  if (!copy) fail('usage: provenance.mjs finish <guard.json> <kept-dir> <copy-dir> [1 = emission only]');
  const guard = JSON.parse(fs.readFileSync(guardPath, 'utf8'));
  // An emission-only repack leaves the kept backend project out of the copy.
  const skipped = emissionOnly === '1' ? new Set(['stage-zero']) : new Set();
  const files = root => {
    const out = new Map();
    const walk = relative => {
      for (const entry of fs.readdirSync(path.join(root, relative), { withFileTypes: true })) {
        const file = relative ? `${relative}/${entry.name}` : entry.name;
        if (skipped.has(file)) continue;
        if (entry.isDirectory()) walk(file);
        else if (entry.isFile()) out.set(file, sha256(fs.readFileSync(path.join(root, file))));
      }
    };
    walk('');
    return out;
  };
  const before = files(kept);
  const after = files(copy);
  const changed = [...after.keys()].filter(file => before.get(file) !== after.get(file)).sort();
  const removed = [...before.keys()].filter(file => !after.has(file)).sort();
  const unexpected = [
    ...changed.filter(file => !PACKAGED.includes(file) && !HARNESS_OUTPUTS.some(pattern => pattern.test(file))),
    ...removed,
  ];

  // Unit/shard crate inputs: both sources must start with reference.rs and
  // carry no split markers after it.
  const reference = fs.readFileSync(path.join(kept, 'reference.rs'));
  const shardInputs = (label, file) => {
    const source = fs.readFileSync(file);
    const prefix = source.length >= reference.length && source.subarray(0, reference.length).equals(reference);
    const tail = source.subarray(reference.length).toString('utf8');
    const markers = SPLIT_MARKERS.filter(marker => tail.includes(marker));
    return { source: label, starts_with_reference: prefix, split_markers_after_reference: markers };
  };
  const shards = [
    shardInputs('kept stage-zero.rs', path.join(kept, 'stage-zero.rs')),
    shardInputs('repacked stage-zero.rs', path.join(copy, 'stage-zero.rs')),
  ];
  const shardsUnchanged = shards.every(row => row.starts_with_reference && !row.split_markers_after_reference.length);

  const outputs = Object.fromEntries(
    [...PACKAGED, 'stage-zero.image', 'stage-zero/src/compiler.image', 'stage-zero/Cargo.toml', 'stage-zero/build.rs'].map(file => [
      file,
      { kept_sha256: before.get(file) ?? null, repacked_sha256: after.get(file) ?? null, identical: before.get(file) === after.get(file) },
    ]),
  );
  const verdict = unexpected.length || !shardsUnchanged ? 'REFUSED' : 'OK';
  const record = {
    schema: 'jet-stage0-repack-provenance/v1',
    verdict,
    ...guard,
    kept_dir: kept,
    repacked_dir: copy,
    emission_only: emissionOnly === '1',
    reference_sha256: sha256(reference),
    outputs,
    changed_vs_kept: changed,
    unexpected_changes: unexpected,
    shard_crate_inputs_unchanged: shardsUnchanged,
    shard_crate_inputs: shards,
    written_at: new Date().toISOString(),
  };
  fs.writeFileSync(path.join(copy, 'repack-provenance.json'), JSON.stringify(record, null, 2) + '\n');
  for (const file of PACKAGED) {
    console.error(`repack provenance: ${file} ${outputs[file].identical ? 'byte-identical to' : 'differs from'} the kept run`);
  }
  if (unexpected.length) fail(`the repack changed files other than the packaged sources:\n  ${unexpected.join('\n  ')}`);
  if (!shardsUnchanged) fail(`the unit/shard crate inputs changed: ${JSON.stringify(shards)}`);
} else {
  fail('usage: provenance.mjs guard|finish ...');
}
