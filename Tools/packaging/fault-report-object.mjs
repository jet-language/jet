// The runtime's jet_rt_fault_report as a linkable object, for a native C
// witness linked by the platform linker. A runtime pack imports that name
// (Image/RuntimePack.jet) because Jet's static linker writes it into each
// program image (Image/StaticLink.jet link_fault_report). A link outside Jet's
// linker writes it from the same source: the full report,
// Compiler/JetRuntime/Assets/FaultReport/report.S, assembled with GNU as
// as Tools/agent/gen-runtime-fault-report.mjs assembles it.
import { spawnSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
export const faultReportSource = join(root, 'Compiler/JetRuntime/Assets/FaultReport/report.S');

// Writes DIRECTORY/fault-report.S and DIRECTORY/fault-report.o, whose global
// function jet_rt_fault_report is report.S's code, and returns the object path.
// The empty .note.GNU-stack section keeps the linked program's stack
// non-executable.
export function faultReportObject(directory) {
  const source = join(directory, 'fault-report.S'), object = join(directory, 'fault-report.o');
  writeFileSync(source, `    .text\n    .globl jet_rt_fault_report\n    .type jet_rt_fault_report, @function\njet_rt_fault_report:\n    .include ${JSON.stringify(faultReportSource)}\n    .size jet_rt_fault_report, . - jet_rt_fault_report\n    .section .note.GNU-stack,"",@progbits\n`, { flag: 'wx' });
  const r = spawnSync(join(root, 'Tools/agent/jet-env'), ['as', '--64', '-o', object, source], { encoding: 'utf8' });
  if (r.error || r.status !== 0) throw Error(`fault report assembly: ${r.error?.message ?? r.status}\n${r.stdout ?? ''}${r.stderr ?? ''}`);
  return object;
}
