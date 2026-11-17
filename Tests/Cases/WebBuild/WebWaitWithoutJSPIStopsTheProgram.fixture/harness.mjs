// A page whose browser has no JavaScript Promise Integration: the host reads
// WebAssembly.Suspending and WebAssembly.promising when it loads, so they are
// gone before app.js is. The program's exit status is node's.
delete WebAssembly.Suspending;
delete WebAssembly.promising;
const { jet_main } = await import("./app.js");
const { JetHostExitError } = await import("./jet_dom_runtime.js");
try {
  await jet_main();
} catch (error) {
  if (!(error instanceof JetHostExitError)) throw error;
  process.exitCode = error.status;
}
