import { execFileSync, spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, chmodSync } from "node:fs";
import { resolve } from "node:path";

if (process.argv.length !== 3) {
  throw new Error("usage: service_lifecycle INPUT_ROOT");
}

const input = resolve(process.argv[2]);
const task = process.env.JET_CORPUS_TASK;
const project = resolve("service-project");
const home = resolve("service-home");
const root = resolve("service-root");
rmSync(project, { recursive: true, force: true });
rmSync(home, { recursive: true, force: true });
rmSync(root, { recursive: true, force: true });
cpSync(input, project, { recursive: true });
mkdirSync(home);
mkdirSync(root);
chmodSync(`${project}/bin/systemd-run`, 0o755);
chmodSync(`${project}/bin/systemctl`, 0o755);
const env = {
  ...process.env,
  HOME: home,
  JETPACK_ROOT: root,
  JETPACK_FAKE_SYSTEMD_STATE: `${project}/systemd-state`,
  JETPACK_SERVICE_HEALTH_TIMEOUT_MS: task === "service-lifecycle-readiness-timeout" ? "200" : "5000",
  PATH: `${project}/bin:${process.env.PATH}`,
};
function run(args) {
  return spawnSync(process.env.JET_CORPUS_JETPACK, args, {
    cwd: project,
    env,
    encoding: "utf8",
    timeout: 10000,
  });
}
try {
  if (task === "service-lifecycle-readiness-timeout") {
    const failed = run(["services", "up", "timeout", "--no-color"]);
    if (failed.error) throw failed.error;
    if (failed.status === 0 || !(failed.stdout + failed.stderr).includes("E1261")) throw new Error("readiness timeout did not fail with E1261");
    const lifecycle = readFileSync(`${project}/.jet/services/timeout/lifecycle`, "utf8");
    if (!lifecycle.includes("phase=failed") || !lifecycle.includes("recovery=startup-failed")) throw new Error("failed service lost lifecycle receipt");
    if (existsSync(`${project}/.jet/services/timeout/pid`)) throw new Error("failed service retained pid");
    const childFile = `${project}/.jet/services/timeout/data/child.pid`;
    if (existsSync(childFile)) {
      const child = readFileSync(childFile, "utf8").trim();
      if (existsSync(`/proc/${child}/stat`) && !readFileSync(`/proc/${child}/stat`, "utf8").split(") ", 2)[1].startsWith("Z ")) throw new Error("failed service retained descendant");
    }
    if (!existsSync(`${project}/.jet/services/timeout/supervisor.error`)) throw new Error("failed service lost supervisor receipt");
    console.log("service=failed\nerror=E1261\nlimit=bounded\ndescendants=contained\nreceipt=startup-failed");
  } else {
    if (run(["services", "up", "fixture", "--no-color"]).status !== 0) throw new Error("service up failed");
    const health = run(["services", "health", "fixture", "--json", "--no-color"]);
    if (health.status !== 0 || !["healthy", "linux-systemd-user", "delegated-cgroup"].every((marker) => health.stdout.includes(marker))) throw new Error("service health receipt drifted");
    const waited = run(["services", "wait", "fixture", "--no-color"]);
    if (waited.status !== 0 || !waited.stderr.includes("service `fixture` is ready")) throw new Error("service wait drifted");
    const logs = run(["services", "logs", "fixture", "--no-color"]);
    if (logs.status !== 0 || !logs.stdout.includes("service-started")) throw new Error("service logs drifted");
    if (run(["services", "down", "fixture", "--no-color"]).status !== 0) throw new Error("service down failed");
    const lifecycle = readFileSync(`${project}/.jet/services/fixture/lifecycle`, "utf8");
    if (!lifecycle.includes("phase=stopped") || !lifecycle.includes("recovery=down")) throw new Error("service stop receipt drifted");
    if (existsSync(`${project}/.jet/services/fixture/pid`)) throw new Error("stopped service retained pid");
    console.log("service=ready\nauthority=linux-systemd-user\ncontainment=delegated-cgroup\nreceipt=health-lifecycle\ncleanup=ok");
  }
} finally {
  rmSync(project, { recursive: true, force: true });
  rmSync(home, { recursive: true, force: true });
  rmSync(root, { recursive: true, force: true });
}
