
const { jet_runtime_stop } = await import("./app.js");
for (const code of ["E9999", "E1001"]) {
  try {
    jet_runtime_stop(code, "tests/fixtures/web_unknown_runtime_stop.jet", 9, "host defect", "run", "fn run() {}", 1, 1, "");
    throw new Error("unexpected success");
  } catch (error) {
    if (error?.name !== "JetHostError") throw error;
    if (error.status !== 101 || error.exitCode !== 101) throw error;
    if (error.code !== code) throw error;
    console.log(`${code}:status=${error.status}:name=${error.name}`);
  }
}
