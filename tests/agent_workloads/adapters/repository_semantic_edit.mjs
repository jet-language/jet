import { cpSync, readFileSync, rmSync, writeFileSync } from "node:fs";

if (process.argv.length !== 3) {
  throw new Error("usage: repository_semantic_edit INPUT_ROOT");
}

const project = "project";
cpSync(process.argv[2], project, { recursive: true });
try {
  const source = `${project}/project/examples/main.jet`;
  const edited = readFileSync(source, "utf8")
    .split(/(?<=\n)/)
    .map((line) => {
      if (line.startsWith("fn prepare()")) return line.replace("fn prepare", "fn configure");
      if (line.replace(/[\r\n]+$/, "") === "    prepare()") {
        return line.replace("prepare()", "configure()");
      }
      return line;
    })
    .join("");
  writeFileSync(source, edited);
  process.stdout.write(readFileSync(source));
} finally {
  rmSync(project, { recursive: true, force: true });
}
