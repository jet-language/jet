// The gauntlet driver of Tools/perf/throughput/measure.mjs (#5072 c7): one
// command per export, printing its result as JSON. driver/run.jet is the twin
// on the kit's Measure module.
//
//   node driver.mjs trace FILE | strict FILE IDENTITY | phases TRACE SAMPLES OFFSET
//                   | cpu SAMPLES T... | counters FILE | tree | run | timeout
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import { cpuAt, parseCounters, phaseTable, readTrace, runMeasured, sampleTree } from '../../../perf/throughput/measure.mjs';

const [command, ...args] = process.argv.slice(2);
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
    console.log(JSON.stringify(phaseTable(readTrace(args[0]), read(args[1]), Number(args[2])), null, 2));
  } else if (command === 'cpu') {
    const samples = read(args[0]);
    console.log(JSON.stringify(args.slice(1).map((t) => cpuAt(samples, Number(t)))));
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
  } else {
    console.log('usage: driver trace|strict|phases|cpu|counters|tree|run|timeout ...');
    process.exit(2);
  }
} catch (error) {
  fail(error);
}
