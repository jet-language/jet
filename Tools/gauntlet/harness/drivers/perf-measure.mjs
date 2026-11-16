// The gauntlet driver of Tools/perf/throughput/measure.mjs (#5072 c7): one
// command per export, printing its result as JSON.
// Tools/gauntlet/entries/perf-measure/driver/run.jet is the twin on the kit's
// Measure module.
//
//   node perf-measure.mjs trace FILE | strict FILE IDENTITY
//                         | phases TRACE SAMPLES OFFSET | cpu SAMPLES TIMES
//                         | counters FILE | tree | run | timeout | constants
//
// A value that starts with `-` takes its named form (`--offset -12345.5`,
// `--times -5,0`), as the twin's typed command line reads it; TIMES is a
// comma-separated list.
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import { GNU_TIME, SAMPLE_MS, cpuAt, parseCounters, phaseTable, readTrace, runMeasured, sampleTree } from '../../../perf/throughput/measure.mjs';

const [command, ...words] = process.argv.slice(2);
// The command's positional values, and its named ones (`--name VALUE`).
const named = {};
const args = [];
for (let at = 0; at < words.length; at += 1) {
  if (words[at].startsWith('--')) {
    named[words[at].slice(2)] = words[at + 1];
    at += 1;
  } else {
    args.push(words[at]);
  }
}
const read = (file) => JSON.parse(fs.readFileSync(file, 'utf8'));
const fail = (error) => {
  console.log(`error: ${error instanceof SyntaxError ? 'SyntaxError' : error.message}`);
  process.exit(1);
};

try {
  if (command === 'trace') {
    console.log(JSON.stringify(readTrace(args[0])));
  } else if (command === 'strict') {
    const events = readTrace(args[0], { completedProcess: read(args[1]) });
    console.log(`${events.length} events`);
  } else if (command === 'phases') {
    console.log(JSON.stringify(phaseTable(readTrace(args[0]), read(args[1]), Number(named.offset ?? args[2])), null, 2));
  } else if (command === 'cpu') {
    const samples = read(args[0]);
    console.log(JSON.stringify((named.times ?? args[1]).split(',').map((t) => cpuAt(samples, Number(t)))));
  } else if (command === 'counters') {
    console.log(JSON.stringify(parseCounters(fs.readFileSync(args[0], 'utf8')), null, 2));
  } else if (command === 'tree') {
    const quiet = 'sleep 3 </dev/null >/dev/null 2>&1';
    const child = spawn('sh', ['-c', `${quiet} & ${quiet} & wait`], { stdio: 'ignore' });
    await new Promise((resolve) => setTimeout(resolve, 300));
    const tree = sampleTree(child.pid);
    console.log(`processes ${tree.processes} threads ${tree.threads} rss ${tree.rssBytes > 0} cpu ${tree.cpuNs >= 0}`);
    spawnSync('pkill', ['-P', String(child.pid)]);
    child.kill('SIGKILL');
  } else if (command === 'run') {
    const measured = await runMeasured(['true'], { stdoutPath: 'out.txt', stderrPath: 'err.txt' }).catch((error) => {
      console.log(`files ${fs.existsSync('out.txt')} ${fs.existsSync('err.txt')} ${fs.existsSync('err.txt.time')}`);
      fail(error);
    });
    console.log(`code ${measured.code}`);
  } else if (command === 'timeout') {
    const measured = await runMeasured(['sh', '-c', '(sleep 1; echo late > late.txt) & sleep 5'], { stdoutPath: 'out.txt', stderrPath: 'err.txt', timeoutMs: 300 });
    await new Promise((resolve) => setTimeout(resolve, 1500));
    console.log(`code ${measured.code} signal ${measured.signal} killed ${measured.killed} late ${fs.existsSync('late.txt')}`);
  } else if (command === 'constants') {
    console.log(`sample ${SAMPLE_MS} ms, GNU time ${GNU_TIME.endsWith('/time') && fs.existsSync(GNU_TIME)}`);
  } else {
    console.log('usage: driver trace|strict|phases|cpu|counters|tree|run|timeout|constants ...');
    process.exit(2);
  }
} catch (error) {
  fail(error);
}
