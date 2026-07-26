import { execFile } from "node:child_process";

if (process.argv.length !== 3) {
  throw new Error("usage: git_diff_review INPUT_ROOT");
}

function gitDiff(root) {
  return new Promise((resolve, reject) => {
    execFile(
      "git",
      [
        "-C",
        root,
        "-c",
        "core.quotePath=false",
        "diff",
        "--no-index",
        "--no-renames",
        "--name-status",
        "--",
        "before",
        "after",
      ],
      {
        encoding: "utf8",
        timeout: 5000,
        killSignal: "SIGKILL",
        maxBuffer: 1024 * 1024,
      },
      (error, stdout, stderr) => {
        if (error?.code === 1) {
          resolve(stdout);
        } else {
          reject(new Error(`git diff exit ${error?.code ?? 0}: ${stderr.trim()}`));
        }
      },
    );
  });
}

const kinds = { A: "added", D: "deleted", M: "modified" };
const counts = { A: 0, D: 0, M: 0 };
const rows = [];
for (const line of (await gitDiff(process.argv[2])).split(/\r?\n/)) {
  if (!line) continue;
  const fields = line.split("\t");
  if (fields.length !== 2 || !(fields[0] in kinds)) {
    throw new Error(`bad git name-status row: ${line}`);
  }
  const [status, rawPath] = fields;
  let path;
  if (rawPath.startsWith("before/")) {
    path = rawPath.slice("before/".length);
  } else if (rawPath.startsWith("after/")) {
    path = rawPath.slice("after/".length);
  } else {
    throw new Error(`git path escaped roots: ${rawPath}`);
  }
  counts[status] += 1;
  rows.push(`${path}|${kinds[status]}`);
}

for (const row of rows.sort()) {
  console.log(row);
}
console.log(
  `summary|added=${counts.A}|modified=${counts.M}|deleted=${counts.D}`,
);
